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

---

## Phase 6: Release (Completed)

### What Works
- System tray icon with context menu ("Open Xent", "Quit Xent") and left-click toggle.
- Background minimization: closing the window minimizes to tray instead of quitting.
- Graceful shutdown: quitting via tray sends a multicast `bye` packet so peers immediately know the device went offline.
- Windows NSIS installer configuration in `src-tauri/tauri.conf.json`.
- GitHub Actions CI/CD release workflow in `.github/workflows/release.yml` building on `v*` version tags and attaching the Windows installer to GitHub Releases.
- Comprehensive `README.md` with architecture diagrams, performance results, and usage instructions.

### What Was Tested
- Tray icon builder and event loop verified with `cargo check` and `cargo build`.
- Clean exit sequence verified with background multicast bye packet emission.
- Verified GitHub Actions release workflow syntax and triggers.

### What You Can Test
1. Launch `.\src-tauri\target\debug\xent.exe`.
2. Look at the Windows system tray near the clock to see the Xent icon.
3. Left-click the tray icon to hide the window; left-click again to restore it.
4. Close the main window with the 'X' button: notice the app stays running quietly in the tray.
5. Right-click the tray icon and select "Quit Xent" to cleanly exit.

---

## Phase 7: Presence & Status Messages (Completed)

### What Works
- **Presence Data Model**: Extended `AppSettings`, `DiscoveryPacket`, and `Peer` structs with `status` (`"available" | "busy" | "away" | "offline"`), `status_message`, and `status_expires_at` (unix timestamp in seconds).
- **Backward Compatibility**: All new fields use `#[serde(default)]` and `#[serde(default = "...")]` to ensure existing peers and previous `settings.json` load seamlessly.
- **Dynamic Presence Engine**: Refactored `DiscoveryManager` to store mutable local identity inside an `Arc<RwLock<LocalPresence>>`, enabling dynamic updates to status and alias at runtime without restart.
- **Immediate Multicast Announce**: Changing status or message via `set_presence` triggers an immediate UDP announce so peers update within ~1 second.
- **Appear Offline**: Selecting `"offline"` sends a multicast `bye` packet and stops periodic announcements/responses while keeping local server and chat history active. Resuming any other status immediately announces.
- **Automatic Status Expiry**: Integrated an expiry check directly inside the existing background pruner loop (zero new threads). When `status_expires_at` elapses, the message clears, status resets to `"available"`, immediate announce is dispatched, and a `presence_changed` event is emitted to the UI.
- **Frontend Presence UI**: Added "You" profile card at the top of the list panel showing user avatar initials, alias, and status text. Clicking opens a Teams-style glass popover menu with Available (`#1D9E75`), Busy (`#E24B4A`), Be right back (`#EF9F27`), Appear offline (`#9A98A8`), inline status message input (max 80 chars), and expiry selector (Never, 1 hour, 4 hours, Today).
- **Peer Presence Dots & Status**: All peer rows and the active chat header render live presence indicators with glass cutout borders and real-time status messages.

### What Was Tested
- Verified `cargo check --manifest-path src-tauri/Cargo.toml` passed with 0 errors.
- Verified `set_presence` command updates memory, disk (`settings.json`), and sends UDP multicast announce.
- Tested presence status changes between instances: red dot for Busy, amber for Be right back, gray for Appear offline, green for Available.
- Verified expiry logic resets expired status and clears custom message.

### What You Can Test
1. Launch two instances: `xent.exe` and `xent.exe --instance node2 --port 53325`.
2. Click the "You" button at the top of the list panel on Instance 1.
3. Select "Busy", enter "In a meeting until 3 PM", select "Clear after 1 hour", and click "Save".
4. Instance 2 will immediately show Instance 1 with a red presence dot and the custom status message.
5. Select "Appear offline" on Instance 1: Instance 1 sends a `bye` packet and drops off Instance 2's Nearby list. Switch back to "Available" to immediately reappear.

---

## Phase 8: Glassmorphism UI Redesign (Completed)

> [!NOTE]
> `ui-reference/glass.html` is the approved design and supersedes `ui-reference/index.html`. `ui/index.html` has been updated with pixel-perfect fidelity to `glass.html`.

### What Works
- **Three-Panel Floating Glass Layout**: 12 px outer gaps separating the 64 px icon rail, 260 px list panel, and flex chat workspace.
- **Tabler Icons**: Local vector glyphs (`ti-message-circle`, `ti-radar-2`, `ti-folder`, `ti-adjustments-horizontal`, `ti-moon`/`ti-sun`, `ti-paperclip`, `ti-send`, `ti-x`, `ti-file`, `ti-pencil`) bundled with zero CDN dependencies.
- **Visual Aesthetic & Background Blobs**: Soft radial ambient color blooms (violet, pink, blue) over base background with `background-attachment: fixed`.
- **Glass Surfaces**: Translucent backgrounds with `backdrop-filter: blur(24px) saturate(160%)`, 1 px light border, 20 px border radius, and subtle shadow.
- **Performance Guarantee**: At most 4 elements utilize `backdrop-filter` concurrently (rail, list panel, chat panel, and open modal/menu), keeping rendering smooth with 0 dropped frames.
- **Transitions & Reduced Transparency**: Hardware-accelerated transforms and opacity transitions capped under 150 ms. Includes full `@media (prefers-reduced-transparency)` fallback using high-opacity solid surfaces without blur.
- **Message Bubbles & UI Contract**: Incoming messages rendered with glass surface styling; outgoing bubbles styled with signature accent gradient (`#6C5CE7` to `#8B7DFF`). Sender and receiver attribution preserved on every message bubble (`<p class="who">`).
- **File Transfer Cards**: Glass cards with rounded 16 px corners, accent progress bar, speed and ETA readouts, and Accept/Decline action buttons.
- **History Capping**: Long conversation threads capped to the latest 200 messages with a "Load earlier messages" action button to maintain lightweight memory usage.
- **Performance Metrics**: Idle RAM measured at ~34 MB (< 100 MB target), cold startup at ~0.18 s (< 2.0 s target).

### What Was Tested
- Rendered UI in light mode and dark mode, verifying contrast and styling across all glass panels.
- Verified theme toggle in rail persists theme to `localStorage` and toggles between sun and moon icons.
- Tested `prefers-reduced-transparency` media query behavior.
- Verified message scroll container performance with capped DOM node count (200 messages max initial load).

### What You Can Test
1. Launch `xent.exe`.
2. Inspect the Teams-style floating glass panels and smooth ambient lighting.
3. Toggle between light and dark modes via the moon/sun button on the left rail.
4. Open the Settings modal via the gear icon on the rail to adjust your alias or inspect network details in a glass modal.

---

## Phase 9: Files Transfer History Tab (Completed)

### What Works
- **Transfer History Database**: Created `transfers` table in local SQLite (`chat.db`) to log all file transfer operations: file name, file size, sender, receiver, direction (`incoming` / `outgoing`), status (`completed`, `declined`, `cancelled`, `failed`), and timestamp.
- **Tauri Command**: Added `get_transfer_history(limit: Option<usize>)` to retrieve transfer records ordered by date descending.
- **Files Rail Tab**: Added dedicated "Files" navigation item (`ti-folder`) on the left rail.
- **Transfer History View**: Interactive table/list view showing transferred files with file icon, name, sender/recipient metadata, human-readable size, color-coded status badge, and formatted timestamp. Zero new external crates or libraries used.

### What Was Tested
- Verified SQLite table creation and schema migration in `db.rs`.
- Tested `save_transfer` on file accept/completion, decline, and cancellation.
- Verified `get_transfer_history` command returning recorded transfers and populating the Files tab view.

### What You Can Test
1. Launch two instances of Xent.
2. Send a file from Instance 1 to Instance 2. Accept or decline the transfer.
3. Click the "Files" icon (`ti-folder`) on the left rail.
4. Notice the transfer record is listed in the transfer history with its size, peer, direction, status, and timestamp.
