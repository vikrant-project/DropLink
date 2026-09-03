import SwiftUI
import PhotosUI
import UniformTypeIdentifiers

public struct ContentView: View {
    @StateObject private var discovery: BonjourDiscovery
    @StateObject private var transferSession: TransferSession
    @StateObject private var transferReceiver: TransferReceiver
    
    @State private var stagedURLs: [URL] = []
    @State private var selectedPhotoItems: [PhotosPickerItem] = []
    @State private var isShowingFilePicker = false
    @State private var isShowingScanner = false
    @State private var selectedTab = 0
    
    private let localDevice: DeviceInfo
    
    public init() {
        let name = UIDevice.current.name
        let defaults = UserDefaults.standard
        let persistentId = defaults.string(forKey: "droplink_device_id") ?? {
            let newId = UUID().uuidString
            defaults.set(newId, forKey: "droplink_device_id")
            return newId
        }()
        
        let wifiIP = getLocalWiFiIP()
        let dev = DeviceInfo(
            id: persistentId,
            name: name,
            platform: .ios,
            version: "1.0.0",
            port: 52520,
            fingerprint: persistentId.replacingOccurrences(of: "-", with: "").uppercased(),
            address: wifiIP
        )
        self.localDevice = dev
        _discovery = StateObject(wrappedValue: BonjourDiscovery(localDevice: dev))
        _transferSession = StateObject(wrappedValue: TransferSession(localDevice: dev))
        _transferReceiver = StateObject(wrappedValue: TransferReceiver(localDevice: dev))
    }
    
    public var body: some View {
        TabView(selection: $selectedTab) {
            nearbyView
                .tabItem {
                    Label("Nearby", systemImage: "wifi")
                }
                .tag(0)
            
            historyView
                .tabItem {
                    Label("History", systemImage: "clock.arrow.circlepath")
                }
                .tag(1)
            
            settingsView
                .tabItem {
                    Label("Settings", systemImage: "gearshape")
                }
                .tag(2)
        }
        .preferredColorScheme(.dark)
        .accentColor(Color(red: 0.39, green: 0.40, blue: 0.95))
        .onAppear {
            discovery.startDiscovery()
            transferReceiver.start()
        }
        .onDisappear {
            discovery.stopDiscovery()
            transferReceiver.stop()
        }
        .sheet(isPresented: $transferSession.isTransferring) {
            transferProgressSheet
        }
        .sheet(item: $transferReceiver.incomingPrompt) { prompt in
            incomingPromptSheet(prompt: prompt)
        }
        .sheet(isPresented: $transferReceiver.isReceiving) {
            receivingProgressSheet
        }
        .sheet(isPresented: $isShowingScanner) {
            QRCodeScannerView { ip, port in
                Task {
                    await discovery.directConnect(to: ip, port: port)
                }
            }
        }
    }
    
    private var nearbyView: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.04, green: 0.06, blue: 0.10).ignoresSafeArea()
                
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("Send anything. Anywhere nearby.")
                                .font(.title2.bold())
                                .foregroundColor(.white)
                            Text("Fast, private local transfers between Apple, Android, and Windows.")
                                .font(.subheadline)
                                .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
                        }
                        .padding(.horizontal)
                        
                        // Action buttons
                        HStack(spacing: 12) {
                            PhotosPicker(
                                selection: $selectedPhotoItems,
                                matching: .any(of: [.images, .videos])
                            ) {
                                Label("Photos & Videos", systemImage: "photo.on.rectangle")
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 12)
                                    .background(Color(red: 0.31, green: 0.27, blue: 0.90))
                                    .foregroundColor(.white)
                                    .cornerRadius(10)
                                    .font(.subheadline.bold())
                            }
                            .onChange(of: selectedPhotoItems) { items in
                                Task {
                                    for item in items {
                                        if let data = try? await item.loadTransferable(type: Data.self) {
                                            let temp = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".jpg")
                                            try? data.write(to: temp)
                                            stagedURLs.append(temp)
                                        }
                                    }
                                    selectedPhotoItems = []
                                }
                            }
                            
                            Button {
                                isShowingFilePicker = true
                            } label: {
                                Label("Files", systemImage: "doc")
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 12)
                                    .background(Color(red: 0.12, green: 0.16, blue: 0.23))
                                    .foregroundColor(.white)
                                    .cornerRadius(10)
                                    .font(.subheadline.bold())
                            }
                            .fileImporter(
                                isPresented: $isShowingFilePicker,
                                allowedContentTypes: [.item],
                                allowsMultipleSelection: true
                            ) { result in
                                if let urls = try? result.get() {
                                    for url in urls {
                                        let isSecured = url.startAccessingSecurityScopedResource()
                                        let tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(url.lastPathComponent)
                                        do {
                                            if FileManager.default.fileExists(atPath: tempURL.path) {
                                                try FileManager.default.removeItem(at: tempURL)
                                            }
                                            try FileManager.default.copyItem(at: url, to: tempURL)
                                            stagedURLs.append(tempURL)
                                        } catch {
                                            if let data = try? Data(contentsOf: url) {
                                                try? data.write(to: tempURL)
                                                stagedURLs.append(tempURL)
                                            } else {
                                                stagedURLs.append(url)
                                            }
                                        }
                                        if isSecured {
                                            url.stopAccessingSecurityScopedResource()
                                        }
                                    }
                                }
                            }
                        }
                        .padding(.horizontal)
                        
                        // Scan QR / Direct IP Connect Button
                        Button {
                            isShowingScanner = true
                        } label: {
                            HStack {
                                Image(systemName: "qrcode.viewfinder")
                                    .font(.body.bold())
                                Text("Scan PC QR Code / Direct IP Connect")
                                    .font(.subheadline.bold())
                            }
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 10)
                            .background(Color(red: 0.16, green: 0.20, blue: 0.30))
                            .foregroundColor(Color(red: 0.65, green: 0.70, blue: 1.0))
                            .cornerRadius(10)
                            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color(red: 0.31, green: 0.27, blue: 0.90), lineWidth: 1))
                        }
                        .padding(.horizontal)
                        
                        // Staged files banner
                        if !stagedURLs.isEmpty {
                            VStack(alignment: .leading, spacing: 10) {
                                HStack {
                                    Text("Ready to Send (\(stagedURLs.count) files)")
                                        .font(.caption.bold())
                                        .foregroundColor(Color(red: 0.88, green: 0.90, blue: 1.0))
                                    Spacer()
                                    Button("Clear") { stagedURLs.removeAll() }
                                        .font(.caption)
                                        .foregroundColor(Color(red: 0.51, green: 0.55, blue: 0.97))
                                }
                                
                                ScrollView(.horizontal, showsIndicators: false) {
                                    HStack(spacing: 8) {
                                        ForEach(stagedURLs, id: \.self) { url in
                                            HStack(spacing: 6) {
                                                Image(systemName: "doc")
                                                Text(url.lastPathComponent)
                                                    .lineLimit(1)
                                            }
                                            .font(.caption)
                                            .padding(.horizontal, 10)
                                            .padding(.vertical, 6)
                                            .background(Color(red: 0.12, green: 0.16, blue: 0.23))
                                            .cornerRadius(6)
                                        }
                                    }
                                }
                            }
                            .padding()
                            .background(Color(red: 0.07, green: 0.09, blue: 0.15))
                            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color(red: 0.31, green: 0.27, blue: 0.90), lineWidth: 1))
                            .cornerRadius(12)
                            .padding(.horizontal)
                        }
                        
                        // Nearby Devices Section
                        VStack(alignment: .leading, spacing: 12) {
                            HStack {
                                Text("Available Devices (\(discovery.discoveredDevices.count))")
                                    .font(.headline)
                                    .foregroundColor(.white)
                                Spacer()
                                HStack(spacing: 4) {
                                    Circle().fill(Color(red: 0.39, green: 0.40, blue: 0.95)).frame(width: 6, height: 6)
                                    Text("Scanning (UDP + Bonjour)")
                                        .font(.caption2)
                                        .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
                                }
                            }
                            
                            if discovery.discoveredDevices.isEmpty {
                                VStack(spacing: 12) {
                                    RadarPulseView()
                                        .padding(.top, 20)
                                    Text("Looking for nearby DropLink devices...")
                                        .font(.subheadline)
                                        .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
                                    Text("Make sure DropLink is open on your PC. You can also tap 'Scan PC QR Code / Direct IP Connect' above.")
                                        .font(.caption)
                                        .foregroundColor(Color(red: 0.42, green: 0.45, blue: 0.50))
                                        .multilineTextAlignment(.center)
                                }
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 20)
                            } else {
                                ForEach(discovery.discoveredDevices) { peer in
                                    deviceCard(for: peer)
                                }
                            }
                        }
                        .padding(.horizontal)
                    }
                    .padding(.vertical)
                }
            }
            .navigationTitle("DropLink")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Text(localDevice.name)
                        .font(.caption2.bold())
                        .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color(red: 0.12, green: 0.16, blue: 0.23))
                        .cornerRadius(8)
                }
            }
        }
    }
    
    private func deviceCard(for peer: DeviceInfo) -> some View {
        HStack {
            Image(systemName: peer.platform == .windows ? "laptopcomputer" : "iphone")
                .font(.title2)
                .foregroundColor(Color(red: 0.51, green: 0.55, blue: 0.97))
                .frame(width: 44, height: 44)
                .background(Color(red: 0.12, green: 0.16, blue: 0.23))
                .clipShape(Circle())
            
            VStack(alignment: .leading, spacing: 2) {
                Text(peer.name)
                    .font(.body.bold())
                    .foregroundColor(.white)
                Text("\(peer.platform.displayName) • \(peer.address ?? "192.168.1.5"):\(String(peer.port))")
                    .font(.caption)
                    .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
            }
            
            Spacer()
            
            Button {
                if !stagedURLs.isEmpty {
                    Task {
                        _ = await transferSession.sendFiles(to: peer.address ?? "192.168.1.5", port: peer.port, fileURLs: stagedURLs)
                        stagedURLs.removeAll()
                    }
                } else {
                    isShowingFilePicker = true
                }
            } label: {
                Text(stagedURLs.isEmpty ? "Select & Send" : "Send Now")
                    .font(.subheadline.bold())
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(Color(red: 0.31, green: 0.27, blue: 0.90))
                    .foregroundColor(.white)
                    .cornerRadius(8)
            }
        }
        .padding()
        .background(Color(red: 0.07, green: 0.09, blue: 0.15))
        .cornerRadius(12)
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color(red: 0.12, green: 0.16, blue: 0.23), lineWidth: 1))
    }
    
    private var transferProgressSheet: some View {
        VStack(spacing: 20) {
            Capsule().fill(Color.gray.opacity(0.4)).frame(width: 40, height: 5).padding(.top, 10)
            
            Text("Transferring Files")
                .font(.headline)
                .foregroundColor(.white)
            
            if let t = transferSession.telemetry {
                Text(t.currentFileName)
                    .font(.subheadline.bold())
                    .foregroundColor(.white)
                Text("File \(t.currentFileIndex + 1) of \(t.totalFiles)")
                    .font(.caption)
                    .foregroundColor(Color.gray)
                
                let pct = t.totalBytes > 0 ? Double(t.transferredBytes) / Double(t.totalBytes) : 0.0
                ProgressView(value: pct)
                    .accentColor(Color(red: 0.31, green: 0.27, blue: 0.90))
                    .padding(.horizontal)
                
                let speedMB = String(format: "%.1f MB/s", t.speedBytesPerSec / (1024 * 1024))
                let etaStr = t.estimatedSecondsRemaining.map { "~\($0)s remaining" } ?? "Estimating..."
                Text("\(speedMB) • \(etaStr)")
                    .font(.caption)
                    .foregroundColor(Color.gray)
            }
            
            Spacer()
            
            Button("Cancel Transfer") {
                transferSession.cancel()
            }
            .foregroundColor(.red)
            .padding(.bottom, 20)
        }
        .presentationDetents([.fraction(0.45)])
        .background(Color(red: 0.07, green: 0.09, blue: 0.15))
    }
    
    private func incomingPromptSheet(prompt: IncomingPromptData) -> some View {
        VStack(spacing: 24) {
            Capsule().fill(Color.gray.opacity(0.4)).frame(width: 40, height: 5).padding(.top, 10)
            
            VStack(spacing: 8) {
                Image(systemName: "arrow.down.circle.fill")
                    .font(.system(size: 48))
                    .foregroundColor(Color(red: 0.39, green: 0.40, blue: 0.95))
                
                Text("Incoming Transfer Request")
                    .font(.title3.bold())
                    .foregroundColor(.white)
                
                Text("\(prompt.manifest.sender.name) wants to send you:")
                    .font(.subheadline)
                    .foregroundColor(Color.gray)
                
                Text("\(prompt.manifest.total_files) file\(prompt.manifest.total_files > 1 ? "s" : "") • \(ByteCountFormatter.string(fromByteCount: prompt.manifest.total_size, countStyle: .file))")
                    .font(.headline)
                    .foregroundColor(.white)
            }
            .padding(.horizontal)
            
            VStack(spacing: 6) {
                Text("Verify matching PIN on both screens:")
                    .font(.caption)
                    .foregroundColor(Color.gray)
                
                Text(prompt.sasPin)
                    .font(.system(size: 28, weight: .bold, design: .monospaced))
                    .foregroundColor(Color(red: 0.65, green: 0.70, blue: 1.0))
                    .padding(.horizontal, 24)
                    .padding(.vertical, 10)
                    .background(Color(red: 0.12, green: 0.16, blue: 0.23))
                    .cornerRadius(12)
            }
            
            Spacer()
            
            HStack(spacing: 16) {
                Button("Decline") {
                    prompt.respond(accept: false)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 14)
                .background(Color.red.opacity(0.2))
                .foregroundColor(.red)
                .cornerRadius(12)
                .font(.headline)
                
                Button("Accept Transfer") {
                    prompt.respond(accept: true)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 14)
                .background(Color(red: 0.31, green: 0.27, blue: 0.90))
                .foregroundColor(.white)
                .cornerRadius(12)
                .font(.headline)
            }
            .padding(.horizontal)
            .padding(.bottom, 20)
        }
        .presentationDetents([.fraction(0.55)])
        .background(Color(red: 0.07, green: 0.09, blue: 0.15))
    }
    
    private var receivingProgressSheet: some View {
        VStack(spacing: 20) {
            Capsule().fill(Color.gray.opacity(0.4)).frame(width: 40, height: 5).padding(.top, 10)
            
            Text("Receiving Files...")
                .font(.headline)
                .foregroundColor(.white)
            
            Text(transferReceiver.currentReceivingName)
                .font(.subheadline.bold())
                .foregroundColor(.white)
                .lineLimit(1)
            
            ProgressView(value: transferReceiver.receivingProgress)
                .accentColor(Color(red: 0.31, green: 0.27, blue: 0.90))
                .padding(.horizontal)
            
            Text("\(Int(transferReceiver.receivingProgress * 100))%")
                .font(.caption)
                .foregroundColor(Color.gray)
            
            Spacer()
        }
        .presentationDetents([.fraction(0.35)])
        .background(Color(red: 0.07, green: 0.09, blue: 0.15))
    }
    
    private var historyView: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.04, green: 0.06, blue: 0.10).ignoresSafeArea()
                
                if transferReceiver.receivedFiles.isEmpty {
                    VStack(spacing: 12) {
                        Image(systemName: "folder")
                            .font(.system(size: 48))
                            .foregroundColor(Color.gray)
                        Text("No transfers yet")
                            .font(.headline)
                            .foregroundColor(.white)
                        Text("Sent and received transfers will appear here.")
                            .font(.caption)
                            .foregroundColor(Color.gray)
                    }
                } else {
                    List {
                        ForEach(transferReceiver.receivedFiles) { item in
                            HStack {
                                Image(systemName: "arrow.down.doc.fill")
                                    .foregroundColor(Color(red: 0.39, green: 0.40, blue: 0.95))
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(item.name)
                                        .font(.body.bold())
                                        .foregroundColor(.white)
                                    Text("From \(item.senderName) • \(ByteCountFormatter.string(fromByteCount: item.size, countStyle: .file))")
                                        .font(.caption)
                                        .foregroundColor(Color.gray)
                                }
                                Spacer()
                            }
                            .listRowBackground(Color(red: 0.07, green: 0.09, blue: 0.15))
                        }
                    }
                    .scrollContentBackground(.hidden)
                }
            }
            .navigationTitle("History")
        }
    }
    
    private var settingsView: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.04, green: 0.06, blue: 0.10).ignoresSafeArea()
                List {
                    Section("Device") {
                        HStack {
                            Text("Device Name")
                            Spacer()
                            Text(localDevice.name).foregroundColor(.gray)
                        }
                        HStack {
                            Text("Wi-Fi IP Address")
                            Spacer()
                            Text(localDevice.address ?? "Connecting...").foregroundColor(Color(red: 0.65, green: 0.70, blue: 1.0))
                        }
                        HStack {
                            Text("Platform")
                            Spacer()
                            Text("iOS").foregroundColor(.gray)
                        }
                    }
                    Section("About") {
                        HStack {
                            Text("DropLink Version")
                            Spacer()
                            Text("1.0.0").foregroundColor(.gray)
                        }
                    }
                }
                .scrollContentBackground(.hidden)
            }
            .navigationTitle("Settings")
        }
    }
}
