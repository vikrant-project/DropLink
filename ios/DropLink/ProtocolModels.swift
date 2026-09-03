import Foundation

public enum PlatformType: String, Codable {
    case windows
    case android
    case ios
    case macos
    case linux
    case unknown
    
    public var displayName: String {
        switch self {
        case .windows: return "Windows"
        case .android: return "Android"
        case .ios: return "iOS"
        case .macos: return "macOS"
        case .linux: return "Linux"
        case .unknown: return "Unknown"
        }
    }
}

public struct DeviceInfo: Codable, Identifiable, Hashable {
    public let id: String
    public var name: String
    public let platform: PlatformType
    public let version: String
    public let port: Int
    public let fingerprint: String
    public var address: String?
    
    public init(id: String, name: String, platform: PlatformType, version: String, port: Int, fingerprint: String, address: String? = nil) {
        self.id = id
        self.name = name
        self.platform = platform
        self.version = version
        self.port = port
        self.fingerprint = fingerprint
        self.address = address
    }
}

public struct FileMetadata: Codable, Identifiable {
    public let id: String
    public let name: String
    public let size: Int64
    public let mime_type: String
    public let sha256: String
    public let relative_path: String?
}

public struct TransferManifest: Codable {
    public let session_id: String
    public let sender: DeviceInfo
    public let files: [FileMetadata]
    public let total_size: Int64
    public let total_files: Int
}

public struct PrepareResponse: Codable {
    public let accepted: Bool
    public let reason: String?
    public let resume_offsets: [String: Int64]
}

public struct DiscoveryBeacon: Codable {
    public let magic: String
    public let device: DeviceInfo
    public let timestamp: Int64
}

public struct LiveTransferTelemetry {
    public let currentFileName: String
    public let currentFileIndex: Int
    public let totalFiles: Int
    public let transferredBytes: Int64
    public let totalBytes: Int64
    public let speedBytesPerSec: Double
    public let estimatedSecondsRemaining: Int?
}

public func getLocalWiFiIP() -> String? {
    var address: String?
    var ifaddr: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr else { return nil }
    for ifptr in sequence(first: firstAddr, next: { $0.pointee.ifa_next }) {
        let interface = ifptr.pointee
        let addrFamily = interface.ifa_addr.pointee.sa_family
        if addrFamily == UInt8(AF_INET) {
            let name = String(cString: interface.ifa_name)
            if name == "en0" {
                var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                getnameinfo(interface.ifa_addr, socklen_t(interface.ifa_addr.pointee.sa_len),
                            &hostname, socklen_t(hostname.count),
                            nil, socklen_t(0), NI_NUMERICHOST)
                address = String(cString: hostname)
            }
        }
    }
    freeifaddrs(ifaddr)
    return address
}
