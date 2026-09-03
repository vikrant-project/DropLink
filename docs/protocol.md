# DropLink Wire Protocol Specification v1.0

## 1. Overview
The DropLink Protocol is a secure, decentralized, peer-to-peer local file transfer protocol designed for high-speed interoperability across **Windows**, **Android**, and **iOS** devices on a local area network (LAN) without cloud servers or user accounts.

---

## 2. Transport & Security Layer
* **Transport**: TCP over local Wi-Fi / Hotspot. Standard port `52520` (dynamic port fallback supported).
* **Encryption**: TLS 1.3 with peer-verified ephemeral X.509 certificates.
* **Mutual Authentication**:
  * Each device generates an ephemeral ECDSA P-256 / SHA-256 certificate on launch.
  * Certificate SHA-256 thumbprints are exchanged out-of-band (via discovery packets or QR codes).
  * **Short Authentication String (SAS)**: A deterministic 6-digit numeric PIN is computed on both peers independently via HKDF-SHA256:
    $$\text{IKM} = \min(\text{FP}_A, \text{FP}_B) \parallel \text{":"} \parallel \max(\text{FP}_A, \text{FP}_B)$$
    $$\text{Salt} = \text{"DROPLINK\_SAS\_V1"}$$
    $$\text{OKM} = \text{HKDF-Expand}(\text{PRK}, \text{"droplink-sas-pin"}, 4)$$
    $$\text{PIN} = (\text{OKM}_{u32} \bmod 1,000,000) \rightarrow \text{"XXX YYY"}$$
  * The user visually verifies that the 6-digit PIN on both screens matches before confirming.

---

## 3. Device Discovery Layer
DropLink implements **Hybrid Discovery**:

### 3.1 Primary: mDNS / DNS-SD
* Service type: `_droplink._tcp.local.`
* TXT Records:
  * `id`: Unique device UUID
  * `name`: UTF-8 device display name (e.g., "Vicky's iPhone")
  * `platform`: `windows` | `android` | `ios` | `macos` | `linux`
  * `ver`: Protocol version (`1.0.0`)
  * `fp`: First 16 hex chars of TLS certificate fingerprint

### 3.2 Secondary: UDP Broadcast Fallback
* Broadcast Destination: `255.255.255.255:52520`
* Interval: Every 2.0 seconds
* Payload:
```json
{
  "magic": "DROPLINK_BEACON",
  "device": {
    "id": "c71e8432-613b-488b-b6d4-8d4e0e64f891",
    "name": "Windows PC",
    "platform": "windows",
    "version": "1.0.0",
    "port": 52520,
    "fingerprint": "4F46E56366F110B981F9FAFB1118271F29370B0F191E293B334155E0E7FF1E1B",
    "address": "192.168.1.105"
  },
  "timestamp": 1725350000
}
```

---

## 4. Transfer Endpoints

### 4.1 Ping Device
`GET /api/v1/ping`
* **Response `200 OK`**:
```json
{
  "id": "c71e8432-613b-488b-b6d4-8d4e0e64f891",
  "name": "Windows PC",
  "platform": "windows",
  "version": "1.0.0",
  "port": 52520,
  "fingerprint": "4F46E563..."
}
```

### 4.2 Pair & Mutual Verification
`POST /api/v1/pair`
* **Request Body**:
```json
{
  "device": { ... },
  "session_id": "76ba37b1-2e21-4f10-9cc0-4965ef2596ab",
  "sas_pin": "482 917"
}
```
* **Response `200 OK`**:
```json
{
  "accepted": true,
  "message": "SAS PIN verified successfully.",
  "session_token": "92f3de18-208b-4b13-9022-7935f8b9e64e"
}
```

### 4.3 Prepare Transfer & Range Negotiation
`POST /api/v1/transfer/prepare`
* **Request Body**:
```json
{
  "session_id": "76ba37b1-2e21-4f10-9cc0-4965ef2596ab",
  "sender": { ... },
  "files": [
    {
      "id": "file-101",
      "name": "vacation_video.mp4",
      "size": 1258291200,
      "mime_type": "video/mp4",
      "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }
  ],
  "total_size": 1258291200,
  "total_files": 1
}
```
* **Response `200 OK`**:
```json
{
  "accepted": true,
  "reason": null,
  "resume_offsets": {
    "file-101": 524288000
  }
}
```
*(If the receiver already has a partial `.droplink_part` file of 500 MB on disk, it reports `524288000` so the sender resumes from that exact offset!)*

### 4.4 Chunked Streaming Upload
`POST /api/v1/transfer/upload/{file_id}`
* **Headers**:
  * `Content-Type: application/octet-stream`
  * `x-droplink-offset: 524288000` *(byte offset when resuming)*
* **Body**: Raw binary chunk stream.
* **Server Action**:
  1. Opens `{clean_name}.droplink_part`.
  2. Seeks to `x-droplink-offset`.
  3. Writes incoming stream directly to disk without loading into RAM.
  4. Flushes and computes full SHA-256 hash.
  5. Verifies SHA-256 matches manifest.
  6. Atomically renames `{clean_name}.droplink_part` to `{clean_name}` (applying auto-rename `(1)` if a conflict exists).
* **Response**: `200 OK` ("File received and verified").

### 4.5 Cancel Active Transfer
`POST /api/v1/transfer/cancel/{session_id}`
* **Response**: `200 OK`. Receiver aborts stream, deletes partial temp file, and frees resources.
