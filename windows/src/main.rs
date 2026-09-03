#![windows_subsystem = "windows"]

use anyhow::Result;
use droplink_core::{
    generate_ephemeral_cert, DeviceInfo, DiscoveryEvent, DiscoveryManager,
    Platform, StorageManager, TransferClient, TransferProgress, TransferServer,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use tokio::sync::{broadcast, oneshot};
use tracing::{error, info};
use uuid::Uuid;
use winreg::enums::*;
use winreg::RegKey;
use wry::{DragDropEvent, WebViewBuilder};

const HTML_INDEX: &str = include_str!("../ui/index.html");
const CSS_STYLE: &str = include_str!("../ui/style.css");
const JS_APP: &str = include_str!("../ui/app.js");
const JS_QR: &str = include_str!("../ui/qrcode.js");

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedFileInfo {
    path: String,
    name: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserSettings {
    #[serde(rename = "deviceName")]
    device_name: String,
    #[serde(rename = "downloadDir")]
    download_dir: String,
    #[serde(rename = "autoAccept")]
    auto_accept: bool,
    autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IpcMessage {
    cmd: String,
    payload: serde_json::Value,
}

enum CustomEvent {
    SendToUi(String),
}

struct AppState {
    local_device: Arc<RwLock<DeviceInfo>>,
    settings: Arc<RwLock<UserSettings>>,
    storage: Arc<StorageManager>,
    staged_files: Arc<RwLock<Vec<StagedFileInfo>>>,
    discovered_peers: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    active_prompt_reply: Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>,
    transfer_client: Arc<TransferClient>,
    is_paused: Arc<AtomicBool>,
    is_cancelled: Arc<AtomicBool>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,droplink_core=debug,droplink_windows=debug")
        .init();

    info!("Starting DropLink Windows Desktop Application...");

    // Create Tokio Runtime for async networking engine
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // 1. Determine Local Data Directory & Defaults
    let local_appdata = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:\\DropLinkData"));
    let data_dir = local_appdata.join("DropLink");
    std::fs::create_dir_all(&data_dir)?;

    let user_downloads = dirs_download_dir().unwrap_or_else(|| data_dir.join("Downloads"));
    let droplink_download_dir = user_downloads.join("DropLink");
    std::fs::create_dir_all(&droplink_download_dir)?;

    let computer_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string());
    let initial_settings = UserSettings {
        device_name: format!("{} (PC)", computer_name),
        download_dir: droplink_download_dir.to_string_lossy().to_string(),
        auto_accept: false,
        autostart: false,
    };

    // 2. Initialize Core Storage & Crypto
    let storage = Arc::new(StorageManager::new(&data_dir)?);
    let cert_bundle = generate_ephemeral_cert(&initial_settings.device_name)?;

    let local_ip = droplink_core::get_local_ip().map(|ip| ip.to_string());
    info!("Detected Local Wi-Fi / LAN IP: {:?}", local_ip);

    let local_device = DeviceInfo {
        id: Uuid::new_v4().to_string(),
        name: initial_settings.device_name.clone(),
        platform: Platform::Windows,
        version: "1.0.0".to_string(),
        port: 52520,
        fingerprint: cert_bundle.fingerprint.clone(),
        address: local_ip,
    };

    let local_device_lock = Arc::new(RwLock::new(local_device.clone()));
    let settings_lock = Arc::new(RwLock::new(initial_settings.clone()));
    let staged_files_lock = Arc::new(RwLock::new(Vec::new()));
    let discovered_peers_lock = Arc::new(RwLock::new(HashMap::new()));
    let active_prompt_reply = Arc::new(parking_lot::Mutex::new(None));

    // 3. Start Transfer Receiver Server
    let (server, mut prompt_rx) = TransferServer::new(
        local_device.clone(),
        droplink_download_dir.clone(),
        Arc::clone(&storage),
        cert_bundle,
    );
    let server_arc = Arc::new(server);

    let server_clone = Arc::clone(&server_arc);
    let actual_port = rt.block_on(async {
        server_clone.start(52520).await.unwrap_or(52520)
    });
    info!("Transfer server active on port {}", actual_port);

    // 4. Start Discovery Manager
    let mut disc_device = local_device.clone();
    disc_device.port = actual_port;
    let discovery = Arc::new(DiscoveryManager::new(disc_device));
    let discovery_clone = Arc::clone(&discovery);
    rt.spawn(async move {
        if let Err(e) = discovery_clone.start().await {
            error!("Discovery manager error: {:#}", e);
        }
    });

    // 5. Initialize Transfer Client
    let transfer_client = Arc::new(TransferClient::new(local_device.clone(), Arc::clone(&storage)));

    let app_state = Arc::new(AppState {
        local_device: local_device_lock,
        settings: settings_lock,
        storage: Arc::clone(&storage),
        staged_files: staged_files_lock,
        discovered_peers: discovered_peers_lock,
        active_prompt_reply,
        transfer_client,
        is_paused: Arc::new(AtomicBool::new(false)),
        is_cancelled: Arc::new(AtomicBool::new(false)),
    });

    // 6. Tao Event Loop & Proxy
    let event_loop: EventLoop<CustomEvent> = EventLoopBuilder::<CustomEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // 7. Bridge Discovery Events to UI
    let mut disc_rx = discovery.subscribe();
    let proxy_disc = proxy.clone();
    let peers_map_disc = Arc::clone(&app_state.discovered_peers);
    rt.spawn(async move {
        while let Ok(event) = disc_rx.recv().await {
            match event {
                DiscoveryEvent::DeviceDiscovered(dev) => {
                    peers_map_disc.write().insert(dev.id.clone(), dev.clone());
                    let json = serde_json::json!({
                        "type": "device_discovered",
                        "data": dev
                    }).to_string();
                    let _ = proxy_disc.send_event(CustomEvent::SendToUi(json));
                }
                DiscoveryEvent::DeviceLost(id) => {
                    peers_map_disc.write().remove(&id);
                    let json = serde_json::json!({
                        "type": "device_lost",
                        "data": id
                    }).to_string();
                    let _ = proxy_disc.send_event(CustomEvent::SendToUi(json));
                }
            }
        }
    });

    // 8. Bridge Incoming Transfer Prompts to UI
    let proxy_prompt = proxy.clone();
    let peers_prompt = Arc::clone(&app_state.discovered_peers);
    let active_reply_slot = Arc::clone(&app_state.active_prompt_reply);
    rt.spawn(async move {
        while let Ok(prompt) = prompt_rx.recv().await {
            let sender = prompt.manifest.sender.clone();
            peers_prompt.write().insert(sender.id.clone(), sender.clone());
            let peer_json = serde_json::json!({
                "type": "device_discovered",
                "data": sender
            }).to_string();
            let _ = proxy_prompt.send_event(CustomEvent::SendToUi(peer_json));

            let reply_sender = prompt.reply_tx.lock().take();
            *active_reply_slot.lock() = reply_sender;

            let json = serde_json::json!({
                "type": "incoming_prompt",
                "data": {
                    "sender_name": prompt.manifest.sender.name,
                    "file_count": prompt.manifest.total_files,
                    "total_size": prompt.manifest.total_size,
                    "sas_pin": prompt.sas_pin
                }
            }).to_string();
            let _ = proxy_prompt.send_event(CustomEvent::SendToUi(json));
        }
    });

    // 9. Build Native Window
    let window = WindowBuilder::new()
        .with_title("DropLink — Send anything. Anywhere nearby.")
        .with_inner_size(LogicalSize::new(980.0, 680.0))
        .with_min_inner_size(LogicalSize::new(820.0, 560.0))
        .build(&event_loop)?;

    // Prepare Inlined HTML with CSS and JS
    let full_html = HTML_INDEX
        .replace("<link rel=\"stylesheet\" href=\"style.css\">", &format!("<style>{}</style>", CSS_STYLE))
        .replace("<script src=\"qrcode.js\"></script>", &format!("<script>{}</script>", JS_QR))
        .replace("<script src=\"app.js\"></script>", &format!("<script>{}</script>", JS_APP));

    // 10. Build WebView with Drag & Drop and IPC
    let proxy_ipc = proxy.clone();
    let app_state_ipc = Arc::clone(&app_state);
    let rt_handle = rt.handle().clone();

    let proxy_drag = proxy.clone();
    let app_state_drag = Arc::clone(&app_state);

    let webview = WebViewBuilder::new()
        .with_html(&full_html)
        .with_drag_drop_handler(move |event| {
            if let DragDropEvent::Drop { paths, .. } = event {
                let mut staged = Vec::new();
                for path in paths {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
                        staged.push(StagedFileInfo {
                            path: path.to_string_lossy().to_string(),
                            name,
                            size: meta.len(),
                        });
                    }
                }
                *app_state_drag.staged_files.write() = staged.clone();
                let json = serde_json::json!({
                    "type": "staged_files",
                    "data": { "files": staged }
                }).to_string();
                let _ = proxy_drag.send_event(CustomEvent::SendToUi(json));
            }
            true
        })
        .with_ipc_handler(move |req| {
            if let Ok(msg) = serde_json::from_str::<IpcMessage>(req.body()) {
                handle_ipc_command(&msg, &app_state_ipc, &proxy_ipc, &rt_handle);
            }
        })
        .build(&window)?;

    let webview = Arc::new(parking_lot::Mutex::new(webview));

    // 11. Run Tao Event Loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                info!("DropLink event loop initialized");
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                info!("DropLink closing...");
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(CustomEvent::SendToUi(json_str)) => {
                let js = format!("window.__droplink_on_message({});", json_str);
                if let Some(wv) = webview.try_lock() {
                    let _ = wv.evaluate_script(&js);
                }
            }
            _ => (),
        }
    });
}

fn handle_ipc_command(
    msg: &IpcMessage,
    state: &Arc<AppState>,
    proxy: &EventLoopProxy<CustomEvent>,
    rt: &tokio::runtime::Handle,
) {
    match msg.cmd.as_str() {
        "get_state" => {
            let local_device = state.local_device.read().clone();
            let settings = state.settings.read().clone();
            let history = state.storage.get_history();
            let peers: Vec<DeviceInfo> = state.discovered_peers.read().values().cloned().collect();

            let json = serde_json::json!({
                "type": "init_state",
                "data": {
                    "local_device": local_device,
                    "settings": settings,
                    "history": history,
                    "peers": peers,
                }
            }).to_string();
            let _ = proxy.send_event(CustomEvent::SendToUi(json));
        }

        "select_files" => {
            let files = rfd::FileDialog::new()
                .set_title("DropLink — Select files to send")
                .pick_files();

            if let Some(paths) = files {
                let mut staged = Vec::new();
                for path in paths {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
                        staged.push(StagedFileInfo {
                            path: path.to_string_lossy().to_string(),
                            name,
                            size: meta.len(),
                        });
                    }
                }
                *state.staged_files.write() = staged.clone();
                let json = serde_json::json!({
                    "type": "staged_files",
                    "data": { "files": staged }
                }).to_string();
                let _ = proxy.send_event(CustomEvent::SendToUi(json));
            }
        }

        "select_folder" => {
            let folder = rfd::FileDialog::new()
                .set_title("DropLink — Select folder to send")
                .pick_folder();

            if let Some(dir) = folder {
                let mut staged = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Ok(meta) = std::fs::metadata(&path) {
                                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
                                staged.push(StagedFileInfo {
                                    path: path.to_string_lossy().to_string(),
                                    name,
                                    size: meta.len(),
                                });
                            }
                        }
                    }
                }
                *state.staged_files.write() = staged.clone();
                let json = serde_json::json!({
                    "type": "staged_files",
                    "data": { "files": staged }
                }).to_string();
                let _ = proxy.send_event(CustomEvent::SendToUi(json));
            }
        }

        "select_download_dir" => {
            let folder = rfd::FileDialog::new()
                .set_title("Select DropLink Download Folder")
                .pick_folder();

            if let Some(dir) = folder {
                let path_str = dir.to_string_lossy().to_string();
                state.settings.write().download_dir = path_str.clone();

                let json = serde_json::json!({
                    "type": "init_state",
                    "data": {
                        "settings": state.settings.read().clone(),
                        "history": state.storage.get_history(),
                        "peers": state.discovered_peers.read().values().cloned().collect::<Vec<_>>(),
                    }
                }).to_string();
                let _ = proxy.send_event(CustomEvent::SendToUi(json));
            }
        }

        "send_files" => {
            let peer_id = msg.payload.get("peer_id").and_then(|v| v.as_str()).unwrap_or_default();
            let host = msg.payload.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1").to_string();
            let port = msg.payload.get("port").and_then(|v| v.as_u64()).unwrap_or(52520) as u16;

            let file_paths: Vec<PathBuf> = msg.payload.get("file_paths")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|s| s.as_str()).map(PathBuf::from).collect())
                .unwrap_or_default();

            if file_paths.is_empty() {
                return;
            }

            let peer_name = state.discovered_peers.read()
                .get(peer_id)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| format!("{}:{}", host, port));

            let client = Arc::clone(&state.transfer_client);
            let proxy_transfer = proxy.clone();
            let (prog_tx, mut prog_rx) = broadcast::channel::<TransferProgress>(32);

            let peer_name_clone = peer_name.clone();
            let total_files = file_paths.len();

            // Progress emitter task
            rt.spawn(async move {
                while let Ok(prog) = prog_rx.recv().await {
                    let json = serde_json::json!({
                        "type": "transfer_progress",
                        "data": {
                            "direction": "sent",
                            "peer_name": peer_name_clone,
                            "file_name": prog.current_file_name,
                            "current_file_index": prog.current_file_index,
                            "total_files": total_files,
                            "bytes_transferred": prog.total_bytes_transferred,
                            "total_bytes": prog.total_bytes_overall,
                            "speed": prog.speed_bytes_per_sec,
                            "eta_seconds": prog.estimated_seconds_remaining,
                        }
                    }).to_string();
                    let _ = proxy_transfer.send_event(CustomEvent::SendToUi(json));
                }
            });

            // Upload task
            let proxy_finish = proxy.clone();
            let storage = Arc::clone(&state.storage);
            rt.spawn(async move {
                let res = client.send_files(&host, port, file_paths, Some(prog_tx)).await;
                if let Err(e) = res {
                    error!("Transfer error: {:#}", e);
                }

                let history = storage.get_history();
                let json = serde_json::json!({
                    "type": "transfer_finished",
                    "data": { "history": history }
                }).to_string();
                let _ = proxy_finish.send_event(CustomEvent::SendToUi(json));
            });
        }

        "respond_incoming" => {
            let accepted = msg.payload.get("accepted").and_then(|v| v.as_bool()).unwrap_or(false);
            if let Some(sender) = state.active_prompt_reply.lock().take() {
                let _ = sender.send(accepted);
            }
        }

        "direct_connect" => {
            let host = msg.payload.get("host").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
            let port = msg.payload.get("port").and_then(|v| v.as_u64()).unwrap_or(52520) as u16;
            if !host.is_empty() {
                let client = Arc::clone(&state.transfer_client);
                let peers_map = Arc::clone(&state.discovered_peers);
                let proxy_dc = proxy.clone();
                rt.spawn(async move {
                    if let Ok(mut info) = client.ping_peer(&host, port).await {
                        info.address = Some(host);
                        peers_map.write().insert(info.id.clone(), info.clone());
                        let json = serde_json::json!({
                            "type": "device_discovered",
                            "data": info
                        }).to_string();
                        let _ = proxy_dc.send_event(CustomEvent::SendToUi(json));
                    }
                });
            }
        }

        "toggle_pause" => {
            let cur = state.is_paused.load(Ordering::Relaxed);
            state.is_paused.store(!cur, Ordering::SeqCst);
        }

        "cancel_transfer" => {
            state.is_cancelled.store(true, Ordering::SeqCst);
        }

        "open_folder" => {
            if let Some(path_str) = msg.payload.get("path").and_then(|v| v.as_str()) {
                let p = Path::new(path_str);
                let folder = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
                let _ = open::that(folder);
            }
        }

        "clear_history" => {
            let _ = state.storage.clear_history();
            let json = serde_json::json!({
                "type": "history_updated",
                "data": { "history": Vec::<droplink_core::TransferRecord>::new() }
            }).to_string();
            let _ = proxy.send_event(CustomEvent::SendToUi(json));
        }

        "save_settings" => {
            if let Ok(new_settings) = serde_json::from_value::<UserSettings>(msg.payload.clone()) {
                *state.settings.write() = new_settings.clone();
                state.local_device.write().name = new_settings.device_name.clone();

                // Windows Autostart Registry Integration
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                if let Ok((run_key, _)) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
                    if new_settings.autostart {
                        if let Ok(exe_path) = std::env::current_exe() {
                            let _ = run_key.set_value("DropLink", &exe_path.to_string_lossy().to_string());
                        }
                    } else {
                        let _ = run_key.delete_value("DropLink");
                    }
                }
            }
        }

        _ => {}
    }
}

fn dirs_download_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .map(|p| PathBuf::from(p).join("Downloads"))
}
