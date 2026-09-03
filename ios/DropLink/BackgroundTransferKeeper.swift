import Foundation
import AVFoundation
import UIKit

public final class BackgroundTransferKeeper {
    public static let shared = BackgroundTransferKeeper()
    
    private var audioPlayer: AVAudioPlayer?
    private var activeTransferCount = 0
    private var bgTaskId: UIBackgroundTaskIdentifier = .invalid
    private let queue = DispatchQueue(label: "com.droplink.bgkeeper")
    
    private init() {}
    
    public func startTransferSession() {
        queue.async {
            self.activeTransferCount += 1
            if self.activeTransferCount == 1 {
                self.enableBackgroundExecution()
            }
        }
    }
    
    public func endTransferSession() {
        queue.async {
            self.activeTransferCount = max(0, self.activeTransferCount - 1)
            if self.activeTransferCount == 0 {
                self.disableBackgroundExecution()
            }
        }
    }
    
    private func enableBackgroundExecution() {
        DispatchQueue.main.async {
            UIApplication.shared.isIdleTimerDisabled = true
            
            if self.bgTaskId == .invalid {
                self.bgTaskId = UIApplication.shared.beginBackgroundTask(withName: "DropLinkBackgroundTransfer") {
                    self.disableBackgroundExecution()
                }
            }
        }
        
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playback, mode: .default, options: [.mixWithOthers])
            try session.setActive(true)
            
            let wavData = createSilentAudioWav()
            audioPlayer = try AVAudioPlayer(data: wavData)
            audioPlayer?.numberOfLoops = -1
            audioPlayer?.volume = 0.0
            audioPlayer?.play()
            print("[BackgroundKeeper] Background transfer protection activated.")
        } catch {
            print("[BackgroundKeeper] Failed to configure audio session: \(error)")
        }
    }
    
    private func disableBackgroundExecution() {
        DispatchQueue.main.async {
            UIApplication.shared.isIdleTimerDisabled = false
            
            if self.bgTaskId != .invalid {
                UIApplication.shared.endBackgroundTask(self.bgTaskId)
                self.bgTaskId = .invalid
            }
        }
        
        audioPlayer?.stop()
        audioPlayer = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        print("[BackgroundKeeper] Background transfer protection deactivated.")
    }
}

private func createSilentAudioWav() -> Data {
    var data = Data()
    // RIFF header
    data.append(contentsOf: [0x52, 0x49, 0x46, 0x46]) // "RIFF"
    let fileSize: UInt32 = 36 + 8000
    withUnsafeBytes(of: fileSize.littleEndian) { data.append(contentsOf: $0) }
    data.append(contentsOf: [0x57, 0x41, 0x56, 0x45]) // "WAVE"
    // fmt chunk
    data.append(contentsOf: [0x66, 0x6D, 0x74, 0x20]) // "fmt "
    let fmtSize: UInt32 = 16
    withUnsafeBytes(of: fmtSize.littleEndian) { data.append(contentsOf: $0) }
    let formatTag: UInt16 = 1 // PCM
    withUnsafeBytes(of: formatTag.littleEndian) { data.append(contentsOf: $0) }
    let channels: UInt16 = 1 // Mono
    withUnsafeBytes(of: channels.littleEndian) { data.append(contentsOf: $0) }
    let sampleRate: UInt32 = 8000
    withUnsafeBytes(of: sampleRate.littleEndian) { data.append(contentsOf: $0) }
    let byteRate: UInt32 = 8000
    withUnsafeBytes(of: byteRate.littleEndian) { data.append(contentsOf: $0) }
    let blockAlign: UInt16 = 1
    withUnsafeBytes(of: blockAlign.littleEndian) { data.append(contentsOf: $0) }
    let bitsPerSample: UInt16 = 8
    withUnsafeBytes(of: bitsPerSample.littleEndian) { data.append(contentsOf: $0) }
    // data chunk
    data.append(contentsOf: [0x64, 0x61, 0x74, 0x61]) // "data"
    let dataSize: UInt32 = 8000
    withUnsafeBytes(of: dataSize.littleEndian) { data.append(contentsOf: $0) }
    // 8000 bytes of silence
    data.append(Data(repeating: 128, count: 8000))
    return data
}
