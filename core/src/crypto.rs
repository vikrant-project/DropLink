use anyhow::{Context, Result};
use hkdf::Hkdf;
use rcgen::{CertificateParams, KeyPair, DistinguishedName, DnType};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

pub struct TlsCertificateBundle {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
    pub fingerprint: String,
}

/// Generates an ephemeral self-signed TLS certificate valid for DropLink local peer connections.
pub fn generate_ephemeral_cert(device_name: &str) -> Result<TlsCertificateBundle> {
    let mut params = CertificateParams::new(vec![
        "localhost".to_string(),
        "droplink.local".to_string(),
    ])?;

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, format!("DropLink-{}", device_name));
    dn.push(DnType::OrganizationName, "DropLink Local Network");
    params.distinguished_name = dn;

    let key_pair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = params.self_signed(&key_pair)?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let cert_der = cert.der().to_vec();

    let mut hasher = Sha256::new();
    hasher.update(&cert_der);
    let fingerprint = format!("{:X}", hasher.finalize());

    Ok(TlsCertificateBundle {
        cert_pem,
        key_pem,
        cert_der,
        fingerprint,
    })
}

/// Computes the deterministic 6-digit Short Authentication String (SAS) numeric PIN
/// shared between two peers from their respective certificate fingerprints.
pub fn compute_sas_pin(fp_a: &str, fp_b: &str) -> String {
    // Lexicographically sort to ensure both devices produce the same PIN
    let mut sorted = [fp_a.to_uppercase(), fp_b.to_uppercase()];
    sorted.sort();

    let salt = b"DROPLINK_SAS_V1";
    let ikm = format!("{}:{}", sorted[0], sorted[1]);
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm.as_bytes());

    let mut okm = [0u8; 4];
    hk.expand(b"droplink-sas-pin", &mut okm)
        .expect("4 bytes is valid HKDF length");

    let val = u32::from_be_bytes(okm) % 1_000_000;
    format!("{:03} {:03}", val / 1000, val % 1000)
}

/// Computes the SHA-256 hex digest of an entire file asynchronously using streaming I/O.
pub async fn compute_file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("Failed to open file for SHA-256 calculation: {:?}", path))?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024]; // 64KB chunk buffer

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ephemeral_cert_generation() {
        let cert = generate_ephemeral_cert("TestDevice").expect("Failed to generate cert");
        assert!(!cert.cert_pem.is_empty());
        assert!(!cert.key_pem.is_empty());
        assert_eq!(cert.fingerprint.len(), 64);
    }

    #[test]
    fn test_sas_pin_symmetry() {
        let fp1 = "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899";
        let fp2 = "112233445566778899AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00";

        let pin1 = compute_sas_pin(fp1, fp2);
        let pin2 = compute_sas_pin(fp2, fp1);

        assert_eq!(pin1, pin2);
        assert_eq!(pin1.len(), 7); // 3 digits + space + 3 digits e.g. "123 456"
    }
}
