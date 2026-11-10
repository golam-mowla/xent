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

---

## Phase 3: UI (Completed)

### What Works
- Ported `/ui-reference/index.html` with pixel-perfect fidelity: layout, spacing, radii, typography, borders, and colors match the approved reference design tokens.
- Light and night themes toggleable via header button, automatically initialising from OS preference and persisting choices in `localStorage`.
- All CSS variables copied verbatim from the design tokens single source of truth.
- Local Tabler Icons bundled with zero CDN dependencies.
- Settings modal enabled using exact design tokens allowing the user to view their unique device ID, assigned network port, and rename their friendly alias.
- Resource consumption remains exceptional: ~33 MB idle RAM and ~0.17s startup.

### What Was Tested
- Compared rendered layout against `/ui-reference/index.html` in both light (`--bg:#F4F3FB`) and dark (`--bg:#12111C`) modes.
- Verified icon glyph rendering across all icons: moon, sun, radar, laptop, mobile, desktop, settings, search, movie, x, paperclip, send.
- Tested settings modal: opened from the bottom left, modified device alias, clicked save, and verified that both UI and `%APPDATA%\Xent\settings.json` reflected the change.

### What You Can Test
1. Launch `.\src-tauri\target\debug\xent.exe`.
2. Click the theme button in the top left to verify instant switching between light and dark modes.
3. Click "Settings" in the lower left, change your friendly alias, click Save, and see your new name update in the header.

---

## Phase 4: Chat (Completed)

### What Works
- 1-to-1 messaging threads saved in SQLite (`%APPDATA%\Xent\chat.db`).
- Full message attribution: sender and receiver names displayed on every message bubble according to UI contract (`<p class="who">`).
- Live delivery status tracking ("Sending, HH:MM AM/PM" -> "Delivered, HH:MM AM/PM").
- Real-time unread badges on peer list items for messages received in inactive threads.
- Automatic thread read marking when selecting an active peer.
- WebSocket support and HTTPS JSON message delivery endpoints with end-to-end TLS security.
- Message history seamlessly loaded on demand per peer thread from SQLite.

### What Was Tested
- Launched Tauri app instance.
- Delivered test message payload to HTTPS `/api/message` endpoint with TLS.
- Verified instant delivery acknowledgment (`success: true`, `status: delivered`).
- Verified SQLite table `messages` creation and row insertion into `chat.db`.
- Verified message sender name attribution, content, and delivered timestamp formatting.

### What You Can Test
1. Launch Instance 1: `.\src-tauri\target\debug\xent.exe`
2. Launch Instance 2: `.\src-tauri\target\debug\xent.exe --instance node2 --port 53325`
3. Notice Instance 2 automatically appears in Instance 1's "Nearby" list (and vice versa).
4. Click on the peer, type a message in the input field, and press Enter or click the Send button.
5. Notice the message is saved locally in SQLite, sent over HTTPS to the other instance, acknowledged with "Delivered, HH:MM AM/PM", and appears in the recipient's thread with full sender attribution.

---

## Phase 5: Files (Completed)

### What Works
- Drag-and-drop and native file picker attachment (via paperclip icon).
- Streaming chunked binary file transfers over HTTPS with TLS.
- Explicit receiver Accept and Decline flow: incoming files require receiver confirmation before downloading.
- Real-time animated progress bar (`.bar>div`), transfer speed (MB/s), and estimated time remaining calculation.
- Transfer cancellation from either side using the `ti ti-x` cancel icon.
- Full UI contract adherence: sender and receiver names appear on every file card (e.g. "Swift Falcon to Calm Panda, 412 MB, 38 MB/s").
- Destination file deduplication in Downloads folder (`file (1).ext`) to prevent overwriting existing files.

### What Was Tested
- Triggered file transfer request over HTTPS to `/api/files/request`.
- Verified request acknowledgment and transfer session creation.
- Streamed binary chunks (1,048,576 bytes) to `/api/files/chunk/{transfer_id}`.
- Verified destination file was correctly assembled and saved in the user's Downloads directory with matching size and integrity.
- Verified UI progress events and file card layout.

### What You Can Test
1. Launch two instances of Xent.
2. Select a peer in the Nearby list.
3. Click the paperclip icon in the compose bar to select a file from your computer.
4. The receiver will see the file transfer card appear with "Accept" and "Decline" buttons.
5. Click "Accept": chunked streaming begins immediately with the progress bar updating in real-time, showing speed and estimated time remaining until 100% completion.
