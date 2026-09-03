# DropLink Architectural Design

## 1. System Architecture Diagram

```mermaid
graph TD
    subgraph Shared Core ("droplink-core (Rust)")
        P[Protocol Engine & DTOs]
        C[Crypto & SAS 6-Digit PIN]
        D[Hybrid Discovery: mDNS + UDP 52520]
        S[Axum TLS Transfer Server]
        CL[Reqwest Streaming Client]
        SEC[Path Sanitizer & Sandboxing]
        SM[Transfer State Machine & Speed Tracker]
        DB[History & Trusted Peers Storage]
    end

    subgraph Windows Desktop ("windows/ (Tauri v2 / Wry + Tao)")
        WIN_UI[Obsidian & Electric Indigo Webview2 UI]
        WIN_MAIN[Win32 Main Event Loop & System Tray]
        WIN_IPC[Native IPC Bridge]
        WIN_DRAG[Win32 Drag & Drop Interceptor]
        WIN_INST[Inno Setup / DropLink-Setup.exe]
    end

    subgraph Android App ("android/ (Kotlin + Jetpack Compose)")
        AND_UI[Material 3 Compose UI & Radar Pulse]
        AND_NSD[NsdManager mDNS & UDP Listener]
        AND_SRV[Foreground Service & Live Notifications]
        AND_SAF[Storage Access Framework & MediaStore]
        AND_ENG[OkHttp Chunked Streaming Engine]
    end

    subgraph iOS App ("ios/ (Swift 6 + SwiftUI)")
        IOS_UI[SwiftUI Dark Mode UI & SF Symbols]
        IOS_BON[Network.framework NWBrowser & NWListener]
        IOS_URL[URLSession Background Streaming]
        IOS_PHO[PhotosPicker & File Provider]
        IOS_IPA[Payload/DropLink.app IPA Packager]
    end

    WIN_IPC --> Shared Core
    AND_ENG -.->|REST / TLS 1.3 Protocol| Shared Core
    IOS_URL -.->|REST / TLS 1.3 Protocol| Shared Core
```

---

## 2. Core Engine Subsystems

### 2.1 Discovery Subsystem (`core/src/discovery.rs`)
* Hybrid design guaranteeing discovery across diverse router configurations:
  * **Primary**: Multicast DNS / DNS-SD (`_droplink._tcp`).
  * **Secondary**: UDP broadcast beaconing on port `52520` (useful for corporate or university Wi-Fi where multicast is blocked).
  * **Reaper**: Automatically drops peers if no beacon packet is received within 10 seconds.

### 2.2 Streaming & Range Resume Subsystem (`core/src/transfer.rs`)
* Designed for large files (10 MB to 100 GB+).
* Zero buffering of full files into RAM: utilizes 512 KB chunk streams directly between network sockets and disk.
* Resumable transfers: Receiver reports byte offset of any existing `.droplink_part` file via `PrepareResponse`, allowing sender to resume using HTTP Range headers (`x-droplink-offset`).

### 2.3 Speed Tracking & Telemetry (`core/src/transfer.rs`)
* Sliding 2-second timestamped window of cumulative bytes transferred.
* Yields jitter-free, accurate instant speed metrics (MB/s) and estimated time remaining (ETA).

---

## 3. Platform Integrations

### 3.1 Windows Desktop
* Built with a native Win32 window host (`tao`) and Microsoft Edge Evergreen WebView2 runtime (`wry`).
* Ultra-low memory footprint (~40 MB RAM compared to 250 MB+ on Electron).
* Full native drag-and-drop listener staging dropped files immediately.
* Autostart integration via Windows Registry `Run` keys.
* Clean installation, shortcuts, and uninstaller.

### 3.2 Android
* 100% native Kotlin with declarative Jetpack Compose UI.
* Foreground Service with `FOREGROUND_SERVICE_TYPE_DATA_SYNC` displaying live progress notifications, preventing OS termination.
* Native Storage Access Framework (`ActivityResultContracts.OpenMultipleDocuments`) and Share Sheet intent filter.

### 3.3 iOS
* Native Swift 6 and SwiftUI with Dark Mode and Dynamic Type.
* Zero external third-party dependencies: relies on Apple's first-party `Network.framework`, `CryptoKit`, `PhotosUI`, and `URLSession`.
* Packaged into standard `DropLink.ipa` archive compatible with AltStore, Sideloadly, TrollStore, or Xcode.
