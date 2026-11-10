# Xent ⚡

> **Fast, lightweight, cross-device LAN messenger and file sharing desktop application.**  
> Built with **Tauri 2**, a high-performance **Rust** backend, and a vanilla HTML/CSS/JS frontend. Zero Electron, zero heavy UI frameworks.

[![Release](https://github.com/golam-mowla/xent/actions/workflows/release.yml/badge.svg)](https://github.com/golam-mowla/xent/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## ⚡ Performance Targets & Results

| Metric | Target | Actual Result | Status |
| :--- | :--- | :--- | :--- |
| **Idle Memory (RAM)** | `< 100 MB` | **~33 MB** | ✅ **67% below target** |
| **Cold Startup Time** | `< 2.0 s` | **~0.17 s** | ✅ **11x faster than target** |
| **External Dependencies** | No CDNs / No cloud | **100% Local (Local fonts & icons)** | ✅ Verified |
| **Network Security** | No unverified plain text | **HTTPS + TLS Fingerprint validation** | ✅ Verified |

---

## 🎯 Features

- **Automatic Zero-Config Discovery**: Like LocalSend, discovers nearby devices on your Wi-Fi or LAN using UDP multicast (`224.0.0.167:53317`) and announces device metadata.
- **Dynamic Presence & Offline Detection**: Devices appear in the "Nearby" list upon announcement and automatically disappear after timing out or when sending a shutdown `bye` packet.
- **End-to-End Local HTTPS Security**: Each node dynamically creates an in-memory TLS certificate with SHA-256 fingerprint validation across local requests.
- **Friendly Aliasing & Device IDs**: Generates random friendly names on first launch (e.g., *Radiant Stag*, *Silver Penguin*) with persistent UUID device IDs. Rename anytime in Settings.
- **1-to-1 Messaging Threads**: Persistent chat history stored in local SQLite (`chat.db`), with real-time delivery status indicators (*Sending*, *Delivered*), and unread badges.
- **Streaming Chunked File Sharing**: Send files of any size via chunked streaming with real-time progress bars, MB/s transfer speed, and remaining time estimates.
- **Receiver Accept / Decline Flow**: Prevents unauthorized file downloads by requiring explicit confirmation from the recipient before transfer begins.
- **Cancellation Support**: Sender or receiver can cancel file transfers in-flight with instant resource cleanup.
- **Strict UI Contract**: Pixel-perfect implementation of the approved design tokens, supporting seamless light and night themes, remembering user choices.
- **System Tray Integration**: Background minimization to the system tray with a right-click menu and quick left-click toggle.

---

## 🛠️ Tech Stack

- **Backend**: Rust, Tauri 2, Tokio async runtime, Axum, Rusqlite, Rustls, Rcgen, Socket2.
- **Frontend**: Vanilla HTML5, CSS3, JavaScript. Bundled local Tabler Icons font (no external CDN calls).
- **Storage**: SQLite (`%APPDATA%\Xent\chat.db`) and JSON configuration (`%APPDATA%\Xent\settings.json`).

---

## 🚀 Getting Started

### Prerequisites

- **Windows 10 / 11** (or Linux / macOS)
- **Rust** (stable toolchain)
- **Node.js** (v18+)

### Development

1. **Clone the repository**:
   ```bash
   git clone https://github.com/golam-mowla/xent.git
   cd xent
   ```

2. **Install frontend dependencies**:
   ```bash
   npm install
   ```

3. **Run in development mode**:
   ```bash
   npm run tauri dev
   # or directly with cargo
   cd src-tauri
   cargo run
   ```

---

## 🧪 Testing Multi-Instance Discovery Locally

You can test discovery, chat, and file transfers between multiple instances on a single machine using isolated instances and ports:

```bash
# Terminal 1: First Instance
.\src-tauri\target\debug\xent.exe

# Terminal 2: Second Instance (isolated config & port)
.\src-tauri\target\debug\xent.exe --instance node2 --port 53325
```

Both instances will automatically discover each other over UDP multicast, exchange TLS certificate fingerprints, and allow real-time chatting and file sharing.

---

## 📦 Building the Installer

Build the NSIS Windows installer locally:
```bash
npx @tauri-apps/cli build
```
The output installer will be located at:
`src-tauri/target/release/bundle/nsis/Xent_0.1.0_x64-setup.exe`.

---

## 🤝 CI / CD Release

A GitHub Actions workflow is configured in `.github/workflows/release.yml`. Pushing a version tag (e.g. `v0.1.0`) triggers an automated build on `windows-latest` that compiles the installer and attaches it to the GitHub Release.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
