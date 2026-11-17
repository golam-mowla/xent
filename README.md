# Xent ⚡

> **Fast, lightweight, cross-device LAN messenger and file sharing desktop application.**  
> Built with **Tauri 2**, a high-performance **Rust** backend, and a vanilla HTML/CSS/JS frontend. Zero Electron, zero heavy UI frameworks.

[![Release](https://github.com/golam-mowla/xent/actions/workflows/release.yml/badge.svg)](https://github.com/golam-mowla/xent/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## ⚡ Performance Targets & Results

| Metric | Target | Actual Result | Status |
| :--- | :--- | :--- | :--- |
| **Idle Memory (RAM)** | `< 100 MB` | **~34 MB** | ✅ **66% below target** |
| **Cold Startup Time** | `< 2.0 s` | **~0.18 s** | ✅ **11x faster than target** |
| **External Dependencies** | No CDNs / No cloud | **100% Local (Tabler icons & local fonts)** | ✅ Verified |
| **Network Security** | No unverified plain text | **HTTPS + TLS Fingerprint validation** | ✅ Verified |
| **Glass Surfaces** | Max 4 blur elements | **<= 4 concurrent `backdrop-filter` panels** | ✅ Verified |

---

## 🎯 Features

- **ChatGPT-Inspired Minimalist Desktop UI**: Replaced heavy glassmorphism with a clean, typography-focused design inspired by ChatGPT's desktop interface (neutral `#FFFFFF`/`#212121` palettes, subtle 1px borders, Inter typography, 240px responsive left sidebar).
- **Four Dedicated Workspaces**:
  - **Messages**: ChatGPT-style centered conversation layout (`max-width: 800px`), date separators, hover action bar (Reply, React, Copy, Edit, Delete), inline file transfer cards, and fixed bottom composer with auto-growing textarea.
  - **Nearby Devices**: Replaces bulky cards with a clean list showing device type icons, OS badges, connection quality, online status, favorites, and quick actions ("Message", "Send Files").
  - **Send Files**: Dedicated file-sharing workspace with drag-and-drop zone, file and folder picker (`pick_folder_to_send`), staged files management, recipient picker, and batch transfer execution.
  - **Transfers**: Professional transfer manager with tabs (All, Incoming, Outgoing), filters (Active, Completed, Failed/Cancelled), live progress bars, speed and ETA readouts, and SQLite transfer history clearing.
- **Custom Context Menu**: Right-click context menus across messages, nearby peers, and transfer rows.
- **Teams-Style Presence & Dynamic Status**: Rich status indicators (*Available*, *Busy*, *Be right back*, *Appear offline*) with custom status messages (up to 80 characters) and configurable auto-clearing expiration (*Never*, *1 hour*, *4 hours*, *Today*).
- **Instant Presence Broadcasting**: Dynamic UDP multicast announcements propagate presence changes in ~1 second across the local network without restarting.
- **Appear Offline Mode**: Emits a multicast `bye` packet and suppresses broadcasts while keeping local server and chat capabilities intact.
- **Teams-Style Glassmorphism UI**: Floating translucent glass panels with 12 px gaps (64 px rail, 260 px list, flex chat), soft ambient radial blooms, bundled Tabler icons, and light/dark theme persistence.
- **High-Performance Rendering**: Strictly adheres to glassmorphism performance rules (maximum 4 concurrent `backdrop-filter` elements, no animated blurs, lightweight transitions <= 150 ms, message cap to latest 200 items with "Load earlier" pagination, and full `@media (prefers-reduced-transparency)` fallback).
- **Files Transfer History Tab**: Dedicated Files navigation view on the rail backed by a persistent SQLite `transfers` table logging all incoming and outgoing file transfers (completed, declined, cancelled, failed).
- **Automatic Zero-Config Discovery**: Like LocalSend, discovers nearby devices on your Wi-Fi or LAN using UDP multicast (`224.0.0.167:53317`) and announces device metadata.
- **Dynamic Presence & Offline Detection**: Devices appear in the "Nearby" list upon announcement and automatically disappear after timing out or when sending a shutdown `bye` packet.
- **End-to-End Local HTTPS Security**: Each node dynamically creates an in-memory TLS certificate with SHA-256 fingerprint validation across local requests.
- **Friendly Aliasing & Device IDs**: Generates random friendly names on first launch (e.g., *Radiant Stag*, *Silver Penguin*) with persistent UUID device IDs. Rename anytime in Settings.
- **1-to-1 Messaging Threads**: Persistent chat history stored in local SQLite (`chat.db`), with real-time delivery status indicators (*Sending*, *Delivered*), sender attribution (`<p class="who">`), and unread badges.
- **Streaming Chunked File Sharing**: Send files of any size via chunked streaming with real-time progress bars, MB/s transfer speed, and remaining time estimates.
- **Receiver Accept / Decline Flow**: Prevents unauthorized file downloads by requiring explicit confirmation from the recipient before transfer begins.
- **Cancellation Support**: Sender or receiver can cancel file transfers in-flight with instant resource cleanup.
- **Custom 3D Origami Neon App Icon**: Custom-branded 3D origami "X" paper airplane icon bundled across all platforms (Windows `.ico`, macOS `.icns`, Linux PNGs, web favicon, and UI rail).
- **In-App GitHub Release Updater**: One-click update check and automatic background release discovery querying GitHub Releases API. Features release notes preview, real-time chunked download progress bar (MB / total MB, percentage), and seamless "Restart & Install" execution.
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
`src-tauri/target/release/bundle/nsis/Xent_0.2.3_x64-setup.exe`.

---

## 🔧 Troubleshooting & Network Configuration

If nearby devices are not appearing automatically or transfers fail, check the following:

### 1. Windows Network Profile: Private vs. Public
- Windows blocks inbound LAN traffic on **Public** networks.
- Open **Windows Settings > Network & Internet > Wi-Fi (or Ethernet)**.
- Click on your active connection and ensure the Network Profile is set to **Private network**.

### 2. Windows Defender Firewall
- When launched for the first time, Windows prompts to allow Xent on Private networks.
- If you missed the prompt or discovery fails, open **Settings > Network** inside Xent and click **Copy Firewall Command**.
- Alternatively, run PowerShell as Administrator:
  ```powershell
  # Allow Xent binary through Windows Firewall
  netsh advfirewall firewall add rule name="Xent App" dir=in action=allow program="<path-to-xent.exe>" enable=yes profile=any
  # Allow LocalSend & Xent UDP Multicast Discovery
  netsh advfirewall firewall add rule name="Xent Discovery UDP" dir=in action=allow protocol=UDP localport=53317 profile=any
  ```

### 3. Wi-Fi Router Settings: AP / Client Isolation
- **Client / AP Isolation**: Many routers have an option called "AP Isolation" or "Client Isolation" that blocks Wi-Fi devices from seeing or communicating with each other. Disable this option in your router's admin panel.
- **Guest Wi-Fi**: Guest Wi-Fi networks isolate every connected device by default. Ensure both devices are connected to the main Wi-Fi SSID.
- **2.4 GHz vs 5 GHz**: Some routers do not bridge multicast across the 2.4 GHz and 5 GHz bands. Xent includes an automatic subnet scanner (`/24`) and an **Add Device by IP** button to connect across bands without relying on multicast.

---

## 🤝 CI / CD Release

A GitHub Actions workflow is configured in `.github/workflows/release.yml`. Pushing a version tag (e.g. `v0.2.2`) triggers an automated build on `windows-latest` that compiles the installer and attaches it to the GitHub Release.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
