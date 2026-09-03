import Foundation
import Network
import Photos
import Combine

public struct IncomingPromptData: Identifiable {
    public let id = UUID()
    public let manifest: TransferManifest
    public let sasPin: String
    let replyHandler: (Bool) -> Void
    
    public func respond(accept: Bool) {
        replyHandler(accept)
    }
}

public struct ReceivedFileInfo: Identifiable {
    public let id = UUID()
    public let name: String
    public let size: Int64
    public let senderName: String
    public let date: Date
}

public final class TransferReceiver: ObservableObject {
    @Published public var incomingPrompt: IncomingPromptData?
    @Published public var receivedFiles: [ReceivedFileInfo] = []
    @Published public var isReceiving = false
    @Published public var receivingProgress: Double = 0.0
    @Published public var currentReceivingName: String = ""
    
    private var listener: NWListener?
    private let localDevice: DeviceInfo
    private var activeManifest: TransferManifest?
    
    public init(localDevice: DeviceInfo) {
        self.localDevice = localDevice
    }
    
    public func start() {
        do {
            let tcpOptions = NWProtocolTCP.Options()
            tcpOptions.noDelay = true
            tcpOptions.enableKeepalive = true
            let params = NWParameters(tls: nil, tcp: tcpOptions)
            params.allowLocalEndpointReuse = true
            params.serviceClass = .responsiveData
            
            let listener = try NWListener(using: params, on: 52520)
            listener.newConnectionHandler = { [weak self] connection in
                self?.handleConnection(connection)
            }
            listener.start(queue: .global(qos: .userInitiated))
            self.listener = listener
            print("[TransferReceiver] Listening on TCP port 52520")
        } catch {
            print("[TransferReceiver] Failed to start listener: \(error)")
        }
    }
    
    public func stop() {
        listener?.cancel()
        listener = nil
    }
    
    private func handleConnection(_ connection: NWConnection) {
        connection.start(queue: .global(qos: .userInitiated))
        readHttpRequest(connection: connection)
    }
    
    private func readHttpRequest(connection: NWConnection) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] content, _, isComplete, error in
            guard let self = self, let data = content, error == nil else {
                connection.cancel()
                return
            }
            
            // Find \r\n\r\n header delimiter in raw bytes to support binary video files!
            let headerSep = Data([0x0D, 0x0A, 0x0D, 0x0A])
            guard let sepRange = data.range(of: headerSep) else {
                connection.cancel()
                return
            }
            
            let headerBytes = data.subdata(in: 0..<sepRange.lowerBound)
            guard let headerStr = String(data: headerBytes, encoding: .utf8) ?? String(data: headerBytes, encoding: .isoLatin1) else {
                connection.cancel()
                return
            }
            
            let lines = headerStr.components(separatedBy: "\r\n")
            guard let requestLine = lines.first else {
                connection.cancel()
                return
            }
            
            let parts = requestLine.components(separatedBy: " ")
            guard parts.count >= 2 else {
                connection.cancel()
                return
            }
            
            let method = parts[0]
            let path = parts[1]
            
            if method == "GET" && path == "/api/v1/ping" {
                self.handlePing(connection: connection)
            } else if method == "POST" && path == "/api/v1/transfer/prepare" {
                self.handlePrepare(connection: connection, requestData: data, headerLines: lines)
            } else if method == "POST" && path.starts(with: "/api/v1/transfer/upload/") {
                let fileId = path.replacingOccurrences(of: "/api/v1/transfer/upload/", with: "")
                self.handleUpload(connection: connection, fileId: fileId, initialData: data, headerLines: lines)
            } else {
                self.sendHttpResponse(connection: connection, statusCode: 404, body: "{\"error\":\"Not Found\"}")
            }
        }
    }
    
    private func handlePing(connection: NWConnection) {
        if let json = try? JSONEncoder().encode(localDevice) {
            sendHttpResponse(connection: connection, statusCode: 200, bodyData: json)
        } else {
            sendHttpResponse(connection: connection, statusCode: 200, body: "{\"status\":\"ok\"}")
        }
    }
    
    private func handlePrepare(connection: NWConnection, requestData: Data, headerLines: [String]) {
        // Extract body after \r\n\r\n
        let separator = "\r\n\r\n".data(using: .utf8)!
        guard let sepRange = requestData.range(of: separator) else {
            sendHttpResponse(connection: connection, statusCode: 400, body: "{\"accepted\":false,\"reason\":\"Invalid request\"}")
            return
        }
        
        let bodyData = requestData.subdata(in: sepRange.upperBound..<requestData.count)
        guard let manifest = try? JSONDecoder().decode(TransferManifest.self, from: bodyData) else {
            sendHttpResponse(connection: connection, statusCode: 400, body: "{\"accepted\":false,\"reason\":\"Invalid manifest\"}")
            return
        }
        
        self.activeManifest = manifest
        let sasPin = DropLinkCrypto.computeSASPin(fingerprintA: manifest.sender.fingerprint, fingerprintB: localDevice.fingerprint)
        
        DispatchQueue.main.async {
            self.incomingPrompt = IncomingPromptData(
                manifest: manifest,
                sasPin: sasPin,
                replyHandler: { [weak self] accepted in
                    guard let self = self else { return }
                    DispatchQueue.main.async {
                        self.incomingPrompt = nil
                    }
                    if accepted {
                        let response = PrepareResponse(accepted: true, reason: nil, resume_offsets: [:])
                        if let respData = try? JSONEncoder().encode(response) {
                            self.sendHttpResponse(connection: connection, statusCode: 200, bodyData: respData)
                        }
                    } else {
                        let response = PrepareResponse(accepted: false, reason: "Declined by recipient", resume_offsets: [:])
                        if let respData = try? JSONEncoder().encode(response) {
                            self.sendHttpResponse(connection: connection, statusCode: 403, bodyData: respData)
                        }
                    }
                }
            )
        }
    }
    
    private func handleUpload(connection: NWConnection, fileId: String, initialData: Data, headerLines: [String]) {
        let separator = "\r\n\r\n".data(using: .utf8)!
        guard let sepRange = initialData.range(of: separator) else {
            sendHttpResponse(connection: connection, statusCode: 400, body: "{\"error\":\"Invalid upload header\"}")
            return
        }
        
        // Find Content-Length
        var contentLength: Int64 = 0
        for line in headerLines {
            let lower = line.lowercased()
            if lower.starts(with: "content-length:") {
                let valStr = line.components(separatedBy: ":").last?.trimmingCharacters(in: .whitespaces) ?? "0"
                contentLength = Int64(valStr) ?? 0
            }
        }
        
        let fileMeta = activeManifest?.files.first(where: { $0.id == fileId })
        let fileName = fileMeta?.name ?? "received_file_\(UUID().uuidString)"
        let targetSize = fileMeta?.size ?? contentLength
        
        let tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + "_" + fileName)
        let cPath = (tempURL.path as NSString).fileSystemRepresentation
        let fd = open(cPath, O_CREAT | O_WRONLY | O_TRUNC, 0o644)
        
        guard fd >= 0 else {
            sendHttpResponse(connection: connection, statusCode: 500, body: "{\"error\":\"Cannot open destination file\"}")
            return
        }
        
        BackgroundTransferKeeper.shared.startTransferSession()
        
        var bytesWritten: Int64 = 0
        let initialBody = initialData.subdata(in: sepRange.upperBound..<initialData.count)
        if !initialBody.isEmpty {
            initialBody.withUnsafeBytes { rawPtr in
                if let base = rawPtr.baseAddress {
                    var written = 0
                    let count = initialBody.count
                    while written < count {
                        let n = write(fd, base + written, count - written)
                        if n <= 0 { break }
                        written += n
                    }
                }
            }
            bytesWritten += Int64(initialBody.count)
        }
        
        var lastUIUpdateTime = Date()
        
        DispatchQueue.main.async {
            self.isReceiving = true
            self.currentReceivingName = fileName
            self.receivingProgress = targetSize > 0 ? Double(bytesWritten) / Double(targetSize) : 0.0
        }
        
        func receiveNextChunks() {
            if targetSize > 0 && bytesWritten >= targetSize {
                finalizeUpload()
                return
            }
            
            connection.receive(minimumIncompleteLength: 1, maximumLength: 1048576) { [weak self] content, _, isComplete, error in
                guard let self = self else { return }
                
                if let chunk = content, !chunk.isEmpty {
                    chunk.withUnsafeBytes { rawPtr in
                        if let base = rawPtr.baseAddress {
                            var written = 0
                            let count = chunk.count
                            while written < count {
                                let n = write(fd, base + written, count - written)
                                if n <= 0 { break }
                                written += n
                            }
                        }
                    }
                    bytesWritten += Int64(chunk.count)
                    
                    // Throttle UI re-renders to at most 5 times/second
                    let now = Date()
                    if now.timeIntervalSince(lastUIUpdateTime) > 0.20 {
                        lastUIUpdateTime = now
                        let pct = targetSize > 0 ? Double(bytesWritten) / Double(targetSize) : 0.0
                        DispatchQueue.main.async {
                            self.receivingProgress = pct
                        }
                    }
                }
                
                if isComplete || (targetSize > 0 && bytesWritten >= targetSize) {
                    finalizeUpload()
                } else if error != nil {
                    close(fd)
                    BackgroundTransferKeeper.shared.endTransferSession()
                    self.sendHttpResponse(connection: connection, statusCode: 500, body: "{\"error\":\"Upload connection error\"}")
                } else {
                    receiveNextChunks()
                }
            }
        }
        
        func finalizeUpload() {
            close(fd)
            BackgroundTransferKeeper.shared.endTransferSession()
            self.saveReceivedFile(from: tempURL, originalName: fileName, fileSize: bytesWritten)
            self.sendHttpResponse(connection: connection, statusCode: 200, body: "{\"status\":\"ok\"}")
            
            DispatchQueue.main.async {
                self.isReceiving = false
                self.receivingProgress = 1.0
            }
        }
        
        if targetSize > 0 && bytesWritten >= targetSize {
            finalizeUpload()
        } else {
            receiveNextChunks()
        }
    }
    
    private func saveReceivedFile(from tempURL: URL, originalName: String, fileSize: Int64) {
        let sender = activeManifest?.sender.name ?? "PC"
        
        // 1. Save to App Documents Directory (accessible in iOS Files App under DropLink)
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let destURL = docs.appendingPathComponent(originalName)
        
        do {
            if FileManager.default.fileExists(atPath: destURL.path) {
                try FileManager.default.removeItem(at: destURL)
            }
            try FileManager.default.copyItem(at: tempURL, to: destURL)
            print("[TransferReceiver] Saved document to: \(destURL.path)")
        } catch {
            print("[TransferReceiver] Error saving document: \(error)")
        }
        
        // 2. If it's a photo or video, also save to Apple Photos Library!
        let ext = tempURL.pathExtension.lowercased()
        let photoExts = ["jpg", "jpeg", "png", "heic", "gif"]
        let videoExts = ["mp4", "mov", "m4v"]
        
        if photoExts.contains(ext) {
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAssetFromImage(atFileURL: tempURL)
            }
        } else if videoExts.contains(ext) {
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAssetFromVideo(atFileURL: tempURL)
            }
        }
        
        DispatchQueue.main.async {
            self.receivedFiles.insert(ReceivedFileInfo(
                name: originalName,
                size: fileSize,
                senderName: sender,
                date: Date()
            ), at: 0)
        }
    }
    
    private func sendHttpResponse(connection: NWConnection, statusCode: Int, body: String = "", bodyData: Data? = nil) {
        let statusText = statusCode == 200 ? "OK" : (statusCode == 404 ? "Not Found" : "Error")
        let data = bodyData ?? body.data(using: .utf8) ?? Data()
        
        let header = "HTTP/1.1 \(statusCode) \(statusText)\r\nContent-Type: application/json\r\nContent-Length: \(data.count)\r\nConnection: close\r\n\r\n"
        var fullData = header.data(using: .utf8)!
        fullData.append(data)
        
        connection.send(content: fullData, completion: .contentProcessed({ _ in
            connection.cancel()
        }))
    }
}
