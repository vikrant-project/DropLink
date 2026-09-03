import Foundation
import CryptoKit

public enum DropLinkCrypto {
    
    /// Computes the SHA-256 hex string of a file at a given URL asynchronously.
    public static func computeFileSHA256(url: URL) -> String? {
        guard let fileHandle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? fileHandle.close() }
        
        var hasher = SHA256()
        while autoreleasepool(invoking: {
            let chunk = fileHandle.readData(ofLength: 64 * 1024)
            guard !chunk.isEmpty else { return false }
            hasher.update(data: chunk)
            return true
        }) {}
        
        let digest = hasher.finalize()
        return digest.map { String(format: "%02x", $0) }.joined()
    }
    
    /// Computes the shared 6-digit Short Authentication String (SAS) PIN between two peers.
    public static func computeSASPin(fingerprintA: String, fingerprintB: String) -> String {
        let sorted = [fingerprintA.uppercased(), fingerprintB.uppercased()].sorted()
        let salt = "DROPLINK_SAS_V1".data(using: .utf8)!
        let ikm = "\(sorted[0]):\(sorted[1])".data(using: .utf8)!
        
        let prk = SymmetricKey(data: HMAC<SHA256>.authenticationCode(for: ikm, using: SymmetricKey(data: salt)))
        let info = "droplink-sas-pin".data(using: .utf8)!
        
        // HKDF-Expand to 4 bytes
        let okm = HKDF<SHA256>.expand(pseudoRandomKey: prk, info: info, outputByteCount: 4)
        var value: UInt32 = 0
        _ = withUnsafeMutableBytes(of: &value) { buffer in
            okm.withUnsafeBytes { okmBuffer in
                buffer.copyMemory(from: okmBuffer)
            }
        }
        let intVal = UInt32(bigEndian: value) % 1_000_000
        let part1 = intVal / 1000
        let part2 = intVal % 1000
        
        return String(format: "%03d %03d", part1, part2)
    }
}
