import Foundation
import Network
import Combine

public final class BonjourDiscovery: ObservableObject {
    
    @Published public private(set) var discoveredDevices: [DeviceInfo] = []
    
    private var browser: NWBrowser?
    private var bonjourListener: NWListener?
    private var udpListener: NWListener?
    private var beaconTimer: Timer?
    private var reaperTimer: Timer?
    private let localDevice: DeviceInfo
    private var deviceMap: [String: (device: DeviceInfo, lastSeen: Date)] = [:]
    
    public init(localDevice: DeviceInfo) {
        self.localDevice = localDevice
    }
    
    public func startDiscovery() {
        // 1. Start Bonjour Browser
        let descriptor = NWBrowser.Descriptor.bonjour(type: "_droplink._tcp", domain: nil)
        let parameters = NWParameters()
        parameters.includePeerToPeer = true
        
        let browser = NWBrowser(for: descriptor, using: parameters)
        browser.browseResultsChangedHandler = { [weak self] results, _ in
            self?.handleBrowseResults(results)
        }
        browser.start(queue: .main)
        self.browser = browser
        
        // 2. Start Bonjour Advertiser
        startAdvertising()
        
        // 3. Start UDP Broadcast Beacon Listener (Port 52520)
        startUdpListener()
        
        // 4. Start UDP Broadcast Beacon Announcer
        startBeaconTimer()
        
        // 5. Start Stale Peer Reaper
        startReaperTimer()
    }
    
    public func stopDiscovery() {
        browser?.cancel()
        browser = nil
        bonjourListener?.cancel()
        bonjourListener = nil
        udpListener?.cancel()
        udpListener = nil
        beaconTimer?.invalidate()
        beaconTimer = nil
        reaperTimer?.invalidate()
        reaperTimer = nil
    }
    
    // Direct manual connection (via scanned QR code or typed IP e.g. 192.168.1.5)
    @discardableResult
    public func directConnect(to host: String, port: Int = 52520) async -> Bool {
        guard let url = URL(string: "http://\(host):\(port)/api/v1/ping") else { return false }
        var req = URLRequest(url: url)
        req.timeoutInterval = 4.0
        
        do {
            let (data, response) = try await URLSession.shared.data(for: req)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200 else { return false }
            var dev = try JSONDecoder().decode(DeviceInfo.self, from: data)
            dev.address = host
            let finalDev = dev
            
            await MainActor.run {
                self.deviceMap = self.deviceMap.filter { (_, item) in
                    if let existingIp = item.device.address, existingIp == host { return false }
                    if item.device.name == finalDev.name && item.device.platform == finalDev.platform { return false }
                    return true
                }
                self.deviceMap[finalDev.id] = (finalDev, Date())
                self.discoveredDevices = self.deviceMap.values.map { $0.device }.sorted(by: { $0.name < $1.name })
            }
            
            self.sendDirectBeacon(to: host, port: port)
            return true
        } catch {
            return false
        }
    }
    
    private func sendDirectBeacon(to host: String, port: Int) {
        let targetHost = NWEndpoint.Host(host)
        guard let targetPort = NWEndpoint.Port(rawValue: UInt16(port)) else { return }
        let conn = NWConnection(host: targetHost, port: targetPort, using: .udp)
        
        let beacon = DiscoveryBeacon(
            magic: "DROPLINK_BEACON",
            device: localDevice,
            timestamp: Int64(Date().timeIntervalSince1970)
        )
        guard let data = try? JSONEncoder().encode(beacon) else { return }
        
        conn.stateUpdateHandler = { state in
            if state == .ready {
                conn.send(content: data, completion: .contentProcessed({ _ in
                    conn.cancel()
                }))
            }
        }
        conn.start(queue: .global())
    }
    
    private func startAdvertising() {
        do {
            let tcpOptions = NWProtocolTCP.Options()
            let params = NWParameters(tls: nil, tcp: tcpOptions)
            params.includePeerToPeer = true
            
            let listener = try NWListener(using: params)
            listener.service = NWListener.Service(
                name: "DropLink-\(localDevice.name)",
                type: "_droplink._tcp"
            )
            listener.start(queue: .main)
            self.bonjourListener = listener
        } catch {
            print("Failed to start Bonjour listener: \(error)")
        }
    }
    
    private func startUdpListener() {
        do {
            let params = NWParameters.udp
            params.allowLocalEndpointReuse = true
            let listener = try NWListener(using: params, on: 52520)
            
            listener.newConnectionHandler = { [weak self] connection in
                connection.start(queue: .global())
                self?.readUdpPacket(connection)
            }
            listener.start(queue: .main)
            self.udpListener = listener
        } catch {
            print("Failed to start UDP listener on 52520: \(error)")
        }
    }
    
    private func readUdpPacket(_ connection: NWConnection) {
        connection.receiveMessage { [weak self] content, _, _, error in
            guard let self = self, let data = content, error == nil else { return }
            
            if let beacon = try? JSONDecoder().decode(DiscoveryBeacon.self, from: data) {
                if beacon.magic == "DROPLINK_BEACON" && beacon.device.id != self.localDevice.id {
                    var dev = beacon.device
                    
                    // Extract sender IP address
                    if case let .hostPort(host, _) = connection.endpoint {
                        let ipStr = host.debugDescription
                            .replacingOccurrences(of: "%en0", with: "")
                            .replacingOccurrences(of: "%pdp_ip0", with: "")
                            .components(separatedBy: "%").first ?? ""
                        dev.address = ipStr
                    }
                    
                    let finalDev = dev
                    DispatchQueue.main.async {
                        // Strict deduplication: Remove any old entry with the same IP or same name+platform
                        self.deviceMap = self.deviceMap.filter { (_, item) in
                            if let existingIp = item.device.address, let newIp = finalDev.address, !newIp.isEmpty, existingIp == newIp {
                                return false
                            }
                            if item.device.name == finalDev.name && item.device.platform == finalDev.platform {
                                return false
                            }
                            return true
                        }
                        self.deviceMap[finalDev.id] = (finalDev, Date())
                        self.discoveredDevices = self.deviceMap.values.map { $0.device }.sorted(by: { $0.name < $1.name })
                    }
                }
            }
        }
    }
    
    private func startBeaconTimer() {
        beaconTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.sendUdpBeacon()
        }
    }
    
    private func startReaperTimer() {
        reaperTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            let now = Date()
            var changed = false
            self.deviceMap = self.deviceMap.filter { (_, item) in
                if now.timeIntervalSince(item.lastSeen) > 12.0 {
                    changed = true
                    return false
                }
                return true
            }
            if changed {
                self.discoveredDevices = self.deviceMap.values.map { $0.device }.sorted(by: { $0.name < $1.name })
            }
        }
    }
    
    private func sendUdpBeacon() {
        var targets = [NWEndpoint.Host("255.255.255.255")]
        
        // Also broadcast to directed local subnet so devices detect iPhone immediately
        if let ip = localDevice.address, let lastDot = ip.lastIndex(of: ".") {
            let subnetBroadcast = String(ip[..<lastDot]) + ".255"
            targets.append(NWEndpoint.Host(subnetBroadcast))
        } else {
            targets.append(NWEndpoint.Host("192.168.1.255"))
            targets.append(NWEndpoint.Host("192.168.0.255"))
        }
        
        let beacon = DiscoveryBeacon(
            magic: "DROPLINK_BEACON",
            device: localDevice,
            timestamp: Int64(Date().timeIntervalSince1970)
        )
        guard let data = try? JSONEncoder().encode(beacon) else { return }
        
        for host in targets {
            let conn = NWConnection(host: host, port: NWEndpoint.Port(rawValue: 52520)!, using: .udp)
            conn.stateUpdateHandler = { state in
                if state == .ready {
                    conn.send(content: data, completion: .contentProcessed({ _ in
                        conn.cancel()
                    }))
                }
            }
            conn.start(queue: .global())
        }
    }
    
    private func handleBrowseResults(_ results: Set<NWBrowser.Result>) {
        for result in results {
            if case let .service(name, _, _, _) = result.endpoint {
                if name.contains("DropLink-") && !name.contains(localDevice.name) {
                    let cleanName = name.replacingOccurrences(of: "DropLink-", with: "")
                    
                    DispatchQueue.main.async {
                        // If device already discovered via UDP with full IP, do not add duplicate Bonjour card!
                        let alreadyExists = self.deviceMap.values.contains { $0.device.name == cleanName }
                        if !alreadyExists {
                            let dev = DeviceInfo(
                                id: name,
                                name: cleanName,
                                platform: .unknown,
                                version: "1.0.0",
                                port: 52520,
                                fingerprint: ""
                            )
                            self.deviceMap[name] = (dev, Date())
                            self.discoveredDevices = self.deviceMap.values.map { $0.device }.sorted(by: { $0.name < $1.name })
                        }
                    }
                }
            }
        }
    }
}
