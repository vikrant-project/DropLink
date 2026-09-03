use crate::protocol::{DeviceInfo, DiscoveryBeacon};
use anyhow::{Context, Result};
use parking_lot::RwLock;
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    DeviceDiscovered(DeviceInfo),
    DeviceLost(String), // device_id
}

pub fn get_local_ip() -> Option<IpAddr> {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                if !addr.ip().is_loopback() && !addr.ip().is_unspecified() {
                    return Some(addr.ip());
                }
            }
        }
        for target in ["192.168.1.1:80", "192.168.0.1:80", "10.0.0.1:80"] {
            if socket.connect(target).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    if !addr.ip().is_loopback() && !addr.ip().is_unspecified() {
                        return Some(addr.ip());
                    }
                }
            }
        }
    }
    None
}

pub struct DiscoveryManager {
    local_device: Arc<RwLock<DeviceInfo>>,
    discovered: Arc<RwLock<HashMap<String, (DeviceInfo, Instant)>>>,
    event_tx: broadcast::Sender<DiscoveryEvent>,
    shutdown_tx: broadcast::Sender<()>,
}

impl DiscoveryManager {
    pub fn new(local_device: DeviceInfo) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            local_device: Arc::new(RwLock::new(local_device)),
            discovered: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            shutdown_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DiscoveryEvent> {
        self.event_tx.subscribe()
    }

    pub fn get_discovered_devices(&self) -> Vec<DeviceInfo> {
        let lock = self.discovered.read();
        lock.values().map(|(dev, _)| dev.clone()).collect()
    }

    pub fn update_local_device(&self, update: DeviceInfo) {
        *self.local_device.write() = update;
    }

    /// Starts both the UDP beacon broadcaster and listener.
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let listener_self = Arc::clone(&self);
        let announcer_self = Arc::clone(&self);

        // Spawn listener
        tokio::spawn(async move {
            if let Err(e) = listener_self.run_listener().await {
                error!("Discovery listener error: {:#}", e);
            }
        });

        // Spawn announcer
        tokio::spawn(async move {
            if let Err(e) = announcer_self.run_announcer().await {
                error!("Discovery announcer error: {:#}", e);
            }
        });

        // Spawn reaper for stale devices
        let reaper_self = Arc::clone(&self);
        tokio::spawn(async move {
            reaper_self.run_reaper().await;
        });

        Ok(())
    }

    async fn run_listener(&self) -> Result<()> {
        let port = DiscoveryBeacon::DEFAULT_PORT;
        
        // Create socket with socket2 to configure reuse_address
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        #[cfg(not(windows))]
        let _ = socket.set_reuse_port(true);
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;

        let bind_addr: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        socket.bind(&bind_addr.into())
            .with_context(|| format!("Failed to bind discovery UDP socket on port {}", port))?;

        let std_socket: std::net::UdpSocket = socket.into();
        let udp = UdpSocket::from_std(std_socket)?;
        info!("Discovery listener listening on UDP port {}", port);

        let mut buf = [0u8; 4096];
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Discovery listener shutting down");
                    break;
                }
                res = udp.recv_from(&mut buf) => {
                    match res {
                        Ok((len, peer_addr)) => {
                            if let Ok(beacon) = serde_json::from_slice::<DiscoveryBeacon>(&buf[..len]) {
                                if beacon.magic == DiscoveryBeacon::MAGIC {
                                    self.handle_incoming_beacon(beacon, peer_addr);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Discovery recv error: {}", e);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_incoming_beacon(&self, beacon: DiscoveryBeacon, peer_addr: SocketAddr) {
        let my_id = self.local_device.read().id.clone();
        if beacon.device.id == my_id {
            // Ignore own beacons
            return;
        }

        let mut device = beacon.device;
        // Inject peer's actual IP address from the packet
        device.address = Some(peer_addr.ip().to_string());

        let mut lock = self.discovered.write();
        let peer_ip = peer_addr.ip().to_string();
        
        // Strict deduplication: Remove any old key that had the same IP or same name+platform
        let mut old_key = None;
        for (k, (existing, _)) in lock.iter() {
            if k == &device.id || 
               (existing.address.as_deref() == Some(&peer_ip)) ||
               (existing.name == device.name && existing.platform == device.platform) {
                old_key = Some(k.clone());
                break;
            }
        }
        if let Some(k) = old_key {
            lock.remove(&k);
        }

        lock.insert(device.id.clone(), (device.clone(), Instant::now()));

        info!("Discovered/Updated device: {} ({}) at {:?}", device.name, device.platform, device.address);
        let _ = self.event_tx.send(DiscoveryEvent::DeviceDiscovered(device));
    }

    async fn run_announcer(&self) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.set_broadcast(true)?;

        let target: SocketAddr = format!("255.255.255.255:{}", DiscoveryBeacon::DEFAULT_PORT).parse()?;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Discovery announcer shutting down");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(2)) => {
                    let dev = self.local_device.read().clone();
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

                    let beacon = DiscoveryBeacon {
                        magic: DiscoveryBeacon::MAGIC.to_string(),
                        device: dev,
                        timestamp: now,
                    };

                    if let Ok(payload) = serde_json::to_vec(&beacon) {
                        let _ = socket.send_to(&payload, target).await;
                    }
                }
            }
        }

        Ok(())
    }

    async fn run_reaper(&self) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let timeout = Duration::from_secs(10);

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = tokio::time::sleep(Duration::from_secs(3)) => {
                    let mut to_remove = Vec::new();
                    {
                        let lock = self.discovered.read();
                        for (id, (_, last_seen)) in lock.iter() {
                            if last_seen.elapsed() > timeout {
                                to_remove.push(id.clone());
                            }
                        }
                    }

                    if !to_remove.is_empty() {
                        let mut lock = self.discovered.write();
                        for id in to_remove {
                            lock.remove(&id);
                            info!("Device lost (timeout): {}", id);
                            let _ = self.event_tx.send(DiscoveryEvent::DeviceLost(id));
                        }
                    }
                }
            }
        }
    }

    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }
}
