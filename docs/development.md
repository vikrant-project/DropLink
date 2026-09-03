# DropLink Development & Build Guide

## 1. Prerequisites

### Windows Desktop
* Rust 1.80+ (`cargo`, `rustc`) with `x86_64-pc-windows-msvc` target
* Microsoft Visual Studio Build Tools 2022/2026 (C++ workload)
* Microsoft Edge WebView2 (built into Windows 10/11)

### Android
* Java JDK 17+ (e.g. Eclipse Adoptium OpenJDK)
* Android SDK / Command-line tools (API 26 to 35)
* Python 3.10+

### iOS
* Python 3.10+ (for cross-platform IPA packaging)
* macOS with Xcode 15+ (for native Apple Developer code signing and App Store submission)

---

## 2. Building Core Engine & Tests

### Run Unit Tests
```bash
cd core
cargo test
```

### Run Cross-Platform Integration & Security Tests
```bash
cargo run --package droplink-tests
```
Executes all 7 automated test suites:
1. Protocol Serialization & DTOs
2. Cryptographic SAS 6-Digit PIN Derivation
3. Path Traversal Attack Defense
4. Windows Reserved Names Sanitization
5. Conflict Auto-Rename
6. Live Peer-to-Peer Transfer & Range Resume
7. Large File Streaming I/O (25 MB+ benchmark)

---

## 3. Building Windows Desktop Application

### Standalone / Portable Binary
```bash
cargo build --release --package droplink-windows
```
Output: `target/release/droplink-windows.exe` -> `dist/DropLink-Portable.exe`

### Windows Installer (`DropLink-Setup.exe`)
```bash
cargo build --release --package droplink-installer
```
Output: `target/release/droplink-installer.exe` -> `dist/DropLink-Setup.exe`

---

## 4. Building Android Application

### Direct Package Build
```bash
python android/scripts/build_apk.py
```
Output: `dist/DropLink.apk`

### Standard Gradle Build (Android Studio / CI)
```bash
cd android
./gradlew assembleRelease
```

---

## 5. Building iOS Application

### Sideload-Ready IPA Build (AltStore / Sideloadly / TrollStore)
```bash
python ios/scripts/build_ipa.py
```
Output: `dist/DropLink.ipa`

### macOS Xcode Archive (App Store / TestFlight CI)
```bash
bash ios/scripts/build_macos_archive.sh
```
Output: `dist/DropLink.ipa` (Signed with Xcode configuration)
