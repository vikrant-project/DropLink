import SwiftUI
import AVFoundation

public struct QRCodeScannerView: View {
    @Environment(\.dismiss) private var dismiss
    let onCodeScanned: (String, Int) -> Void
    
    @State private var manualIP: String = ""
    @State private var manualPort: String = "52520"
    @State private var isConnecting = false
    @State private var errorMessage: String?
    
    public init(onCodeScanned: @escaping (String, Int) -> Void) {
        self.onCodeScanned = onCodeScanned
    }
    
    public var body: some View {
        NavigationStack {
            ZStack {
                Color(red: 0.04, green: 0.06, blue: 0.10).ignoresSafeArea()
                
                VStack(spacing: 20) {
                    // Camera Scanner Preview Box
                    ZStack {
                        CameraPreviewRepresentable { code in
                            handleScannedString(code)
                        }
                        .frame(height: 280)
                        .cornerRadius(16)
                        .overlay(
                            RoundedRectangle(cornerRadius: 16)
                                .stroke(Color(red: 0.39, green: 0.40, blue: 0.95), lineWidth: 2)
                        )
                        
                        // Targeting guide
                        RoundedRectangle(cornerRadius: 12)
                            .stroke(Color.white.opacity(0.6), style: StrokeStyle(lineWidth: 2, dash: [10]))
                            .frame(width: 180, height: 180)
                    }
                    .padding(.horizontal)
                    
                    Text("Point camera at DropLink QR code on your Windows PC")
                        .font(.caption)
                        .foregroundColor(Color(red: 0.61, green: 0.64, blue: 0.69))
                    
                    Divider().background(Color(red: 0.12, green: 0.16, blue: 0.23))
                    
                    // Manual Wi-Fi IP Entry
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Or Connect Directly via Wi-Fi IP:")
                            .font(.caption.bold())
                            .foregroundColor(.white)
                        
                        HStack(spacing: 8) {
                            TextField("e.g. 192.168.1.5", text: $manualIP)
                                .textFieldStyle(.plain)
                                .padding(12)
                                .background(Color(red: 0.07, green: 0.09, blue: 0.15))
                                .cornerRadius(8)
                                .foregroundColor(.white)
                                .font(.system(.body, design: .monospaced))
                                .keyboardType(.decimalPad)
                            
                            TextField("52520", text: $manualPort)
                                .frame(width: 70)
                                .textFieldStyle(.plain)
                                .padding(12)
                                .background(Color(red: 0.07, green: 0.09, blue: 0.15))
                                .cornerRadius(8)
                                .foregroundColor(.white)
                                .font(.system(.body, design: .monospaced))
                                .keyboardType(.numberPad)
                            
                            Button("Connect") {
                                connectManual()
                            }
                            .padding(.horizontal, 16)
                            .padding(.vertical, 12)
                            .background(Color(red: 0.31, green: 0.27, blue: 0.90))
                            .foregroundColor(.white)
                            .cornerRadius(8)
                            .font(.subheadline.bold())
                        }
                        
                        if let err = errorMessage {
                            Text(err)
                                .font(.caption2)
                                .foregroundColor(.red)
                        }
                    }
                    .padding(.horizontal)
                    
                    Spacer()
                }
                .padding(.top)
            }
            .navigationTitle("Pair Device")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button("Close") { dismiss() }
                        .foregroundColor(.white)
                }
            }
        }
    }
    
    private func handleScannedString(_ raw: String) {
        // Expected format: droplink://192.168.1.5:52520?name=...
        var ip = raw
        var port = 52520
        
        if raw.contains("droplink://") {
            let clean = raw.replacingOccurrences(of: "droplink://", with: "")
            let hostPart = clean.components(separatedBy: "?").first ?? clean
            let parts = hostPart.components(separatedBy: ":")
            ip = parts[0]
            if parts.count > 1, let p = Int(parts[1]) {
                port = p
            }
        } else if raw.contains(":") {
            let parts = raw.components(separatedBy: ":")
            ip = parts[0]
            if parts.count > 1, let p = Int(parts[1]) {
                port = p
            }
        }
        
        onCodeScanned(ip, port)
        dismiss()
    }
    
    private func connectManual() {
        let cleanIP = manualIP.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !cleanIP.isEmpty else {
            errorMessage = "Please enter a valid IP address"
            return
        }
        let port = Int(manualPort.trimmingCharacters(in: .whitespacesAndNewlines)) ?? 52520
        onCodeScanned(cleanIP, port)
        dismiss()
    }
}

// Camera Preview UIViewControllerRepresentable
struct CameraPreviewRepresentable: UIViewControllerRepresentable {
    let onCodeDetected: (String) -> Void
    
    func makeUIViewController(context: Context) -> ScannerViewController {
        let vc = ScannerViewController()
        vc.onCodeDetected = onCodeDetected
        return vc
    }
    
    func updateUIViewController(_ uiViewController: ScannerViewController, context: Context) {}
}

class ScannerViewController: UIViewController, AVCaptureMetadataOutputObjectsDelegate {
    var onCodeDetected: ((String) -> Void)?
    private var captureSession: AVCaptureSession?
    
    override func viewDidLoad() {
        super.viewDidLoad()
        setupCamera()
    }
    
    private func setupCamera() {
        let session = AVCaptureSession()
        guard let videoCaptureDevice = AVCaptureDevice.default(for: .video),
              let videoInput = try? AVCaptureDeviceInput(device: videoCaptureDevice),
              session.canAddInput(videoInput) else { return }
        
        session.addInput(videoInput)
        
        let metadataOutput = AVCaptureMetadataOutput()
        if session.canAddOutput(metadataOutput) {
            session.addOutput(metadataOutput)
            metadataOutput.setMetadataObjectsDelegate(self, queue: DispatchQueue.main)
            metadataOutput.metadataObjectTypes = [.qr]
        }
        
        let previewLayer = AVCaptureVideoPreviewLayer(session: session)
        previewLayer.frame = view.layer.bounds
        previewLayer.videoGravity = .resizeAspectFill
        view.layer.addSublayer(previewLayer)
        
        DispatchQueue.global(qos: .userInitiated).async {
            session.startRunning()
        }
        self.captureSession = session
    }
    
    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if let sublayers = view.layer.sublayers {
            for layer in sublayers {
                if let previewLayer = layer as? AVCaptureVideoPreviewLayer {
                    previewLayer.frame = view.bounds
                }
            }
        }
    }
    
    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        if captureSession?.isRunning == true {
            captureSession?.stopRunning()
        }
    }
    
    func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput metadataObjects: [AVMetadataObject], from connection: AVCaptureConnection) {
        if let metadataObject = metadataObjects.first as? AVMetadataMachineReadableCodeObject,
           let stringValue = metadataObject.stringValue {
            captureSession?.stopRunning()
            AudioServicesPlaySystemSound(SystemSoundID(kSystemSoundID_Vibrate))
            onCodeDetected?(stringValue)
        }
    }
}
