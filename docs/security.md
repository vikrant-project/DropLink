# DropLink Security & Cryptographic Architecture

## 1. Threat Model & Design Principles
DropLink operates in zero-trust local network environments, including:
* Public Wi-Fi hotspots (coffee shops, airports, hotel networks)
* Multi-tenant office Wi-Fi
* Direct ad-hoc Wi-Fi hotspots between mobile devices and laptops

**Core Protections**:
1. **Never send files in plaintext**.
2. **Never trust remote metadata** (always sanitize paths, filenames, and lengths).
3. **Never allow path traversal or filesystem escapes**.
4. **Prevent Man-in-the-Middle (MITM) attacks** without central Certificate Authorities.
5. **Guarantee file integrity** via end-to-end cryptographic hashing (SHA-256).

---

## 2. Cryptographic Architecture

### 2.1 Ephemeral TLS 1.3 Transport
* DropLink generates an ephemeral **ECDSA P-256** self-signed certificate upon application launch.
* Keys are held in memory only and never stored in plain files.
* Forward secrecy is guaranteed: sessions cannot be retroactively decrypted even if an endpoint is compromised later.

### 2.2 Short Authentication String (SAS) Verification
Because local networks lack public CA certificates, DropLink uses **Short Authentication Strings (SAS)** derived via **HKDF-SHA256**:
1. When two devices connect, they exchange their TLS certificate fingerprints.
2. The fingerprints are sorted lexicographically:
   $$\text{IKM} = \min(\text{FP}_A, \text{FP}_B) \parallel \text{":"} \parallel \max(\text{FP}_A, \text{FP}_B)$$
3. HKDF-Extract and HKDF-Expand compute a 32-bit integer formatted into two 3-digit groups: `"482 917"`.
4. The user verifies this PIN matches on both devices before accepting the first transfer.
5. An active MITM attacker altering either key will result in mismatched PINs, immediately alerting the user!

### 2.3 QR Code Instant Pairing
* When physical proximity allows, one device displays a QR code containing its connection string:
  `droplink://192.168.1.10:52520?name=MyPhone&fp=4F46E563...`
* The scanning peer compares the actual TLS certificate fingerprint during the handshake against the scanned fingerprint. Any mismatch terminates the connection immediately.

---

## 3. Path Traversal & Input Sanitization

### 3.1 Filename Neutralization
Untrusted filenames from peers undergo strict sanitization in `droplink-core::security::sanitize_filename`:
* Path separators (`/`, `\`), null bytes (`\0`), and illegal characters (`:`, `*`, `?`, `"`, `<`, `>`, `|`) are replaced with `_`.
* Control characters (`0x00` - `0x1F`, `0x7F`) are removed.
* Consecutive dot sequences (`..`, `...`, `....`) are collapsed and trimmed.
* Leading and trailing dots or whitespace (which Windows filesystems reject) are stripped.
* Empty filenames default to `unnamed_file`.

### 3.2 Windows Reserved Device Names
Windows kernels reserve specific legacy DOS device names that crash or freeze naive file writers if written to disk (e.g. `CON`, `PRN`, `AUX`, `NUL`, `COM1-COM9`, `LPT1-LPT9`).
DropLink detects these names (case-insensitive base stem) and automatically prefixes them with an underscore (e.g. `CON.txt` -> `_CON.txt`).

### 3.3 Strict Path Sandboxing
The destination path is resolved via `resolve_safe_path`:
* Any path component resolving to `Component::ParentDir` (`..`) triggers an immediate bail error.
* The resolved path is verified to strictly begin with the authorized download directory.

---

## 4. Conflict Resolution & Atomic Writes

1. **Partial File Isolation**: While data is being received, chunks are written to `{filename}.droplink_part`. Other applications cannot mistake an in-progress transfer for a completed file.
2. **Atomic Commit**: Only after the full file is received and verified against its manifest SHA-256 hash is the file renamed to its final name.
3. **Sequential Auto-Rename**: If `photo.jpg` already exists, DropLink inspects `photo (1).jpg`, `photo (2).jpg`, etc., avoiding silent overwrites while ensuring deterministic delivery.
