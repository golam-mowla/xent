# Xent - Progress Log

## Phase 1: Scaffold (Completed)

### What Works
- Initialized Tauri 2 desktop application with Rust backend and vanilla HTML/CSS/JS frontend.
- Bundled Tabler Icons locally in `ui/tabler-icons.min.css` and `ui/fonts/` (no external CDN dependency).
- Random friendly alias generator on first launch (e.g. "Radiant Stag", "Keen Beaver") combining adjectives and animal names.
- Persistent unique device ID (UUID v4) and settings storage in JSON (`%APPDATA%\Xent\settings.json`).
- Isolated multi-instance support using `--instance <ID> --port <PORT>` flags for local testing of multiple devices.
- Startup time is < 0.3 seconds and idle RAM is ~40 MB, comfortably beating our targets (< 2s startup, < 100 MB RAM).
- UI contract preserved: design tokens, exact colors, typography, borders, shadows, and spacing matched to `/ui-reference/index.html`.

### What Was Tested
- Built binary using `cargo build` on Windows.
- Executed `xent.exe` and verified process working set (~40 MB) and responsive startup.
- Verified automatic creation and persistence of `settings.json` on launch.
- Tested launch with `--instance 2 --port 53318` and verified isolated `settings_2.json` creation with distinct device ID and alias.
- Verified light/dark theme toggle matches OS on first load and persists user preference.

### What You Can Test
1. Run `cargo run --manifest-path src-tauri/Cargo.toml` or `.\src-tauri\target\debug\xent.exe`.
2. Inspect `%APPDATA%\Xent\settings.json` to see your generated friendly alias and device ID.
3. Toggle between light and dark themes using the moon/sun button in the top left.

---

## Phase 2: Discovery (Completed)

### What Works
- LocalSend-compatible UDP multicast discovery over `224.0.0.167:53317` with `SO_REUSEADDR` and loopback enabled.
- Automatic device announcement containing device ID, alias, device type, port, protocol version, and TLS certificate fingerprint.
- Unicast/multicast immediate response mechanism so newly announced devices are discovered instantly.
- Background HTTP/HTTPS server on each node running via Axum with self-signed TLS certificates (generated in-memory using `rcgen` and `ring`).
- SHA-256 certificate fingerprint exchange and validation for secure peer-to-peer transport.
- Automatic offline detection: peers that do not send an announcement within 8 seconds (or that emit a `bye` packet) are pruned from the nearby devices list and disappear from "Nearby".
- Dynamic fallback port binding if default port is occupied.

### What Was Tested
- Ran two simultaneous instances on the same host network interface (`192.168.0.105`): Instance 1 on port 53318 ("Radiant Stag") and Instance 2 on port 53325 ("Silver Penguin").
- Verified mutual discovery:
  - Instance 1 immediately logged discovery of `Silver Penguin` at `192.168.0.105:53325`.
  - Instance 2 immediately logged discovery of `Radiant Stag` at `192.168.0.105:53318`.
- Queried HTTPS `/api/info` endpoints on both instances via curl and verified valid JSON responses with TLS fingerprints.
- Terminated Instance 2 and observed Instance 1 automatically log: `[Discovery] Peer offline (timed out): Silver Penguin (...)` after 8 seconds.

### What You Can Test
1. Run Instance 1 in one terminal:
   `.\src-tauri\target\debug\xent.exe`
2. Run Instance 2 in another terminal:
   `.\src-tauri\target\debug\xent.exe --instance node2 --port 53325`
3. Check the console or logs to see both instances discover each other and exchange fingerprints.
4. Kill Instance 2 and watch it disappear from the active peers list after 8 seconds.
