import Foundation
import Combine

public final class TransferSession: ObservableObject {
    
    @Published public var telemetry: LiveTransferTelemetry?
    @Published public var isTransferring: Bool = false
    @Published public var errorMessage: String?
    
    private var session: URLSession?
    private var activeTask: URLSessionUploadTask?
    private var isCancelled = false
    private let localDevice: DeviceInfo
    
    public init(localDevice: DeviceInfo) {
        self.localDevice = localDevice
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 300
        self.session = URLSession(configuration: config)
    }
    
    public func cancel() {
        isCancelled = true
        activeTask?.cancel()
        isTransferring = false
    }
    
    public func sendFiles(to host: String, port: Int, fileURLs: [URL]) async -> Bool {
        isCancelled = false
        await MainActor.run {
            self.isTransferring = true
            self.errorMessage = nil
        }
        
        let sessionId = UUID().uuidString
        var collectedFiles: [FileMetadata] = []
        var calculatedTotalBytes: Int64 = 0
        
        for url in fileURLs {
            let isAccessing = url.startAccessingSecurityScopedResource()
            var size: Int64 = 0
            if let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
               let s = attrs[.size] as? Int64, s > 0 {
                size = s
            } else if let s = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize), s > 0 {
                size = Int64(s)
            } else if let data = try? Data(contentsOf: url) {
                size = Int64(data.count)
            }
            
            let sha256 = DropLinkCrypto.computeFileSHA256(url: url) ?? ""
            collectedFiles.append(FileMetadata(
                id: UUID().uuidString,
                name: url.lastPathComponent,
                size: max(size, 1),
                mime_type: "application/octet-stream",
                sha256: sha256,
                relative_path: nil
            ))
            calculatedTotalBytes += max(size, 1)
            
            if isAccessing {
                url.stopAccessingSecurityScopedResource()
            }
        }
        
        let finalFiles = collectedFiles
        let finalTotalBytes = calculatedTotalBytes
        
        let manifest = TransferManifest(
            session_id: sessionId,
            sender: localDevice,
            files: finalFiles,
            total_size: finalTotalBytes,
            total_files: finalFiles.count
        )
        
        // 1. Prepare Request
        guard let prepareURL = URL(string: "http://\(host):\(port)/api/v1/transfer/prepare") else { return false }
        var prepReq = URLRequest(url: prepareURL)
        prepReq.httpMethod = "POST"
        prepReq.setValue("application/json", forHTTPHeaderField: "Content-Type")
        prepReq.httpBody = try? JSONEncoder().encode(manifest)
        
        do {
            let (data, response) = try await session!.data(for: prepReq)
            guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else {
                await MainActor.run { self.errorMessage = "Receiver rejected the transfer." }
                return false
            }
            
            let prepResp = try JSONDecoder().decode(PrepareResponse.self, from: data)
            guard prepResp.accepted else {
                await MainActor.run { self.errorMessage = prepResp.reason ?? "Declined by receiver." }
                return false
            }
            
            // 2. Stream Each File
            BackgroundTransferKeeper.shared.startTransferSession()
            defer { BackgroundTransferKeeper.shared.endTransferSession() }
            
            var cumulativeBytes: Int64 = 0
            let startTime = Date()
            
            for (idx, meta) in finalFiles.enumerated() {
                if isCancelled { break }
                let url = fileURLs[idx]
                let isAccessing = url.startAccessingSecurityScopedResource()
                let startOffset = prepResp.resume_offsets[meta.id] ?? 0
                cumulativeBytes += startOffset
                
                guard let uploadURL = URL(string: "http://\(host):\(port)/api/v1/transfer/upload/\(meta.id)") else {
                    if isAccessing { url.stopAccessingSecurityScopedResource() }
                    continue
                }
                var uploadReq = URLRequest(url: uploadURL)
                uploadReq.httpMethod = "POST"
                uploadReq.setValue("\(startOffset)", forHTTPHeaderField: "x-droplink-offset")
                
                let (_, uploadResp) = try await session!.upload(for: uploadReq, fromFile: url)
                if isAccessing { url.stopAccessingSecurityScopedResource() }
                guard let httpUpload = uploadResp as? HTTPURLResponse, httpUpload.statusCode == 200 else {
                    await MainActor.run { self.errorMessage = "Upload failed." }
                    return false
                }
                
                cumulativeBytes += meta.size - startOffset
                let elapsed = Date().timeIntervalSince(startTime)
                let speed = elapsed > 0 ? Double(cumulativeBytes) / elapsed : 0.0
                let remaining = finalTotalBytes - cumulativeBytes
                let eta = speed > 1024 ? Int(Double(remaining) / speed) : nil
                
                let currentTransferred = cumulativeBytes
                let currentFileName = meta.name
                let currentIndex = idx
                let totalCount = finalFiles.count
                let currentSpeed = speed
                let currentEta = eta
                
                await MainActor.run {
                    self.telemetry = LiveTransferTelemetry(
                        currentFileName: currentFileName,
                        currentFileIndex: currentIndex,
                        totalFiles: totalCount,
                        transferredBytes: currentTransferred,
                        totalBytes: finalTotalBytes,
                        speedBytesPerSec: currentSpeed,
                        estimatedSecondsRemaining: currentEta
                    )
                }
            }
            
            await MainActor.run { self.isTransferring = false }
            return true
            
        } catch {
            await MainActor.run {
                self.errorMessage = error.localizedDescription
                self.isTransferring = false
            }
            return false
        }
    }
}
