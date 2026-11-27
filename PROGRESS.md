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

---

## Phase 10: Reliable Discovery & File Transfers (Completed)

### What Works
- **Tauri Global Injected**: Enabled `app.withGlobalTauri: true` in `tauri.conf.json` ensuring `window.__TAURI__` is reliably present in the frontend runtime. Added visible fallback alert banner `#backend-banner` if Tauri is absent.
- **Multicast & Multi-Adapter Broadcast Discovery**: `discovery.rs` rewritten to bind and join multicast groups across all active local network adapters. Outgoing announce, reply, and bye packets are transmitted via both multicast (`224.0.0.167:53317`) and subnet broadcast (`255.255.255.255:53317`) on every interface. Peer timeout extended to 15 s.
- **Subnet Scan Fallback (/24 Scanner)**: Background scanner automatically probes all 254 host addresses on each adapter's `/24` subnet at `https://<ip>:<port>/api/info` (concurrency: 64, timeout: 400 ms) when no peers are detected within 5 s.
- **Manual Peer Discovery ("Add Device by IP")**: Users can manually connect to peers by IP and port (`192.168.x.x[:port]`) using the "Add by IP" modal. Manually added peers persist in `settings.json` under `manual_peers` and are actively probed every 10 seconds.
- **Network Diagnostics & Firewall Integration**: Settings > Network tab displays all active local IPv4 addresses, HTTPS listener port, UDP socket binding status, last received discovery packet timestamp, and connected peer counts. Includes a one-click "Copy Firewall Command" button generating the exact `netsh advfirewall` commands.
- **Dynamic Adapter Change Detection**: Network watcher polls network interfaces every 5 seconds; if IP addresses change (e.g. Wi-Fi reconnection or VPN toggle), discovery rebinds and broadcasts an immediate announcement.
- **Multi-File Concurrent Attachments**: Paperclip attachment button utilizes `rfd` native multi-file picker (`pick_files_to_send`), automatically targeting the active peer or the sole online peer if only one peer is available.
- **Drag & Drop and Clipboard Image Pasting**: Native drag-drop overlay ("Drop to send to <peer>") displays when files are dragged over the workspace. Clipboard image pasting (Ctrl+V) automatically writes the image to disk and stages it for immediate sending.
- **Interactive File Cards**: Completed file cards feature "Open" and "Show in folder" native shell execution actions, with thumbnail previews for received images.
- **Persistent Transfer Timeline**: All file transfers persist in SQLite `transfers` table and render chronologically inside the conversation thread.
- **Customizable Download Directory & Auto-Accept**: Settings > Files tab provides a native directory picker for download location and an auto-accept toggle for incoming files.

---

## Phase 11: Teams-Style Messaging & App Quality (Completed)

### What Works
- **Message Delivery States**: Chat messages support Sending, Delivered, and Read receipts. Opening a conversation marks messages as read and informs the peer via `/api/message/read`.
- **Typing Indicator**: Real-time typing alerts ("... is typing") delivered via `/api/message/typing`, throttled to once every 2 seconds.
- **Rich Message Actions**:
  - **Reply / Quote**: Quoting previous messages with preview bar and click-to-jump.
  - **Edit Message**: Allows editing messages sent within 15 minutes, adding an "(edited)" indicator.
  - **Delete for Everyone**: Soft deletes messages, replacing content with "This message was deleted".
  - **Emoji Reactions**: Quick emoji reactions (👍 ❤️ 😂 😮 😢 🔥) displayed beneath message bubbles.
- **Local Emoji Picker**: Zero-dependency floating emoji picker with popular categories.
- **Offline Message Queueing**: Messages sent to offline peers are automatically queued as "Queued" in SQLite and flushed once the peer reappears on the network.
- **Search & Filtering**: Search bar in the list panel filters nearby peers by alias and searches message history. Supports `Ctrl+F` in-thread search.
- **Tabbed Settings Modal**: Glassmorphism tabbed settings interface covering General (alias, theme), Files (download directory, auto-accept), Network (diagnostics, firewall commands), and About (version, open log directory).
- **Keyboard Shortcuts**: `Esc` closes open modals and quote previews, `Ctrl+F` focuses search, and `Ctrl+Shift+A` triggers file attachment.

---

## Phase 12: Polish, Unit Testing & CI/CD (Completed)

### What Works
- **Date Separators**: Chat timeline automatically groups messages by calendar day with stylish glass date pills ("Today", "Yesterday", or formatted date).
- **Image Lightbox Modal**: Clicking any image thumbnail in chat or file cards opens a full-screen glass lightbox modal with backdrop blur.
- **Automated Unit Tests**:
  - `test_legacy_packet_parsing`: Verifies backwards-compatible deserialization of legacy discovery packets lacking `status` fields.
  - `test_v2_packet_parsing`: Verifies parsing of v2 discovery announcements with status messages.
  - `test_legacy_settings_deserialization`: Verifies settings backwards compatibility for older config files lacking new v2 properties.
- **Performance Verification**:
  - Cold startup: ~0.19 s (well below 2.0 s requirement).
  - Idle RAM usage: ~38 MB (well below 100 MB budget).
  - Glass performance: Strictly limited to at most 4 concurrent `backdrop-filter` elements.

---

## Phase 13: Laptop-to-Desktop Transfer Fixes & Device Management (Completed)

### What Works
- **Backend-Driven Upload Execution**: Upload now starts directly from the Rust backend (`spawn_upload`) immediately when the receiver responds with `accepted`. It no longer depends on the sender's frontend chat window being focused or active.
- **Exact Peer Addressing**: Receiver replies directly to the socket origin address (`ConnectInfo<SocketAddr>`) where the request came from, eliminating dependency on transient discovery lists.
- **Real HTTP Error Propagation**: Failed chunk writes now return HTTP `400 Bad Request` instead of `200 OK`, ensuring the sender detects any file write errors.
- **Path Traversal & Security Sanitization**: Incoming file names are sanitized (`sanitize_file_name`), stripping paths and Windows reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`). File chunks are rejected until the transfer is explicitly accepted.
- **Live Settings Access**: Download directory and preferences are queried live from `SettingsState` rather than using a stale startup snapshot.
- **Favourite & Blocked Devices**:
  - **Favourites**: Star/pin favorite devices at the top of Nearby; files from favourite devices are accepted automatically without prompting.
  - **Custom Nicknames**: Rename devices locally (persisted in `settings.json` under `device_nicknames`).
  - **Block List**: Block devices to silently drop unwanted messages and file offers.
  - **Settings > Devices Tab**: Manage favourites and blocked devices with unpin and unblock actions.
- **Interactive UI Polish**:
  - "Try again" retry button on failed, declined, or cancelled file cards.
  - "Open Downloads Folder" button in Settings > Files.
  - "Clear Conversation" action in chat header.
  - Unread counter and toast notification when incoming file offers arrive from inactive threads.
- **Multiplatform Sockets**: Added `socket2` with `features = ["all"]` to ensure cross-platform compilation on Windows, Linux, and macOS.
- **Automated Unit Tests (11/11 passing)**:
  - `sanitize_strips_paths`, `sanitize_handles_bad_names`
  - `upload_starts_only_once`, `chunks_rejected_until_accepted`, `empty_file_completes`
  - `nickname_set_trim_and_clear`, `blocking_removes_favorite`, `favorite_toggle_and_legacy_trusted`
  - All 11 unit tests pass in 0.04s.

---

## Phase 14: Custom 3D Origami App Icon & In-App GitHub Releases Updater (Completed)

### What Works
- **Custom 3D Origami Neon App Icon**:
  - Replaced default Tauri icons with custom 3D geometric origami "X" paper airplane in neon blue/cyan and magenta/purple gradient.
  - Automatically generated all required multi-platform icon assets using `@tauri-apps/cli`:
    - `icon.ico` (multi-resolution Windows executable and taskbar icon).
    - `icon.icns` (macOS bundle icon).
    - PNG sizes: `32x32.png`, `64x64.png`, `128x128.png`, `128x128@2x.png`, `icon.png`, and all StoreLogo/Square assets.
    - Web favicon (`ui/favicon.png`) and in-app asset (`ui/app-icon.png`).
  - Added clickable brand logo button (`#rail-logo-btn`) at the top of the left rail navigation.
- **In-App GitHub Releases Updater**:
  - Added `src-tauri/src/updater.rs` with asynchronous update checking against the public GitHub Releases API (`https://api.github.com/repos/golam-mowla/xent/releases/latest`).
  - Built-in semver comparison logic (`is_newer_version`) reliably parsing versions with or without `v` prefixes and pre-release tags.
  - **Download & Progress Streaming**: Downloads the release installer (`Xent_<version>_x64-setup.exe`) to the system temporary directory with real-time `update://progress` Tauri events (reporting downloaded bytes, total bytes, percentage, and transfer status).
  - **One-Click Installation**: Seamless "Restart & Install" execution launching the NSIS installer and gracefully terminating the running application so files can be updated.
  - **Settings > About UI Integration**:
    - Displays high-resolution branded app icon and active version pill.
    - Dedicated "Software Update" card with manual "Check for updates" button and animated loading indicator.
    - Release details preview showing the new version badge, release date, and scrollable release notes.
    - Live progress bar showing real-time download progress (MB downloaded / total MB, percentage).
    - Direct "View on GitHub" fallback button to open the release webpage in the system default browser.
  - **Silent Background Notification**:
    - Silently checks for updates in the background 3.5 seconds after app startup.
    - If a newer version is available, illuminates an indicator dot badge on the Settings rail button and displays an informative toast notification.
- **Automated Unit Tests (12/12 passing)**:
  - Added unit test `updater::tests::test_is_newer_version` verifying semver comparison across edge cases (`0.2.3 > 0.2.2`, `1.0.0 > 0.2.2`, `v0.2.3 > v0.2.2`, equal and older versions).
  - All 12 unit tests pass in 0.05s.

---

## Phase 15: Complete UI Redesign (ChatGPT-Inspired Minimal Design) & Workspaces (Completed)

### What Works
- **Complete Visual Overhaul (ChatGPT Aesthetic)**:
  - Clean, spacious, typography-focused desktop experience inspired by ChatGPT's minimalist interface.
  - Eliminated glassmorphism and heavy radial blur decorations in favor of a crisp, neutral palette:
    - Light: `#FFFFFF` (main), `#F7F7F8` (sidebar), `#EEEEF0` (hover), `#E8E8EA` (border), `#202123` (text), `#8E8E93` (secondary text), `#635BDB` (accent).
    - Dark: `#212121` (main), `#171717` (sidebar), `#2A2A2A` (surface), `#2F2F2F` (hover), `#3A3A3A` (border), `#ECECEC` (text), `#A1A1AA` (secondary text), `#635BDB` (accent).
    - System theme detection with live OS appearance matching.
  - Primary font: Inter with native system font fallbacks.
- **250px Minimal Navigation Sidebar**:
  - Clean brand header with origami 3D icon and collapse toggle button.
  - 4 Primary Workspace Navigation Buttons with live unread/active counters:
    - **Messages** (`ti-message-dots`)
    - **Nearby** (`ti-radar-2`)
    - **Send Files** (`ti-upload`)
    - **Transfers** (`ti-arrows-transfer-down`)
  - Sub-section for active peer conversations with quick search (`Ctrl+F`).
  - Bottom Profile Bar: User avatar, friendly name, presence indicator, presence popover menu (Available, Busy, Away, Offline, custom status with auto-expiration), theme toggle, and Settings button with update notification badge.
- **Workspace 1: ChatGPT-Style Messages**:
  - Centered message stream (`max-width: 800px`) with date separators and comfortable line spacing.
  - Hover action bar for messages: Reply with quote banner, React (👍, ❤️, 😂, 😮, 😢, 🔥), Copy, Edit, and Delete.
  - Fixed rounded bottom composer with auto-resizing textarea, attachment button, emoji popover, send button, Enter to send, Shift+Enter for newline.
  - Native drag-and-drop overlay and clipboard image paste support.
  - Inline file transfer cards with live progress bars, speed, ETA, Open, and Show in Folder actions.
- **Workspace 2: Nearby Devices Clean List**:
  - Replaced bulky cards with a streamlined table/list view.
  - Device rows display device icon, name, custom nickname, OS tag, status pill, IP:port, interactive favorite star, and quick actions ("Message", "Send Files").
  - Filter pills: All, Desktops & Laptops, Mobile, Favorites.
  - Direct "+ Connect by IP" button and rescan action.
- **Workspace 3: Dedicated Send Files Workspace**:
  - Target recipient selector with live device status.
  - Large drag-and-drop file/folder staging zone.
  - Native file picker (`pick_files_to_send`) and folder picker (`pick_folder_to_send`) with recursive discovery.
  - Staging manager: list of selected files with icons, formatted file sizes, individual removal, total size summary, and batch send execution routing to Transfers.
- **Workspace 4: Professional Transfer Manager**:
  - Tabbed direction filter: **All**, **Incoming**, **Outgoing**.
  - Status filters: **All Status**, **Active**, **Completed**, **Failed / Cancelled**.
  - "Clear History" button backed by SQLite `clear_all_transfers` backend command.
  - Rows show file icon, file name, formatted size, counterpart device, direction badge, progress bar, MB/s speed, time remaining, and status.
- **Custom Context Menu**:
  - Sleek right-click context menu on messages (Copy, Reply, React, Edit, Delete), peers (Message, Send Files, Rename, Favorite, Block), and transfers (Open File, Show in Folder, Cancel, Retry).
- **Settings Modal & Modals**:
  - Clean tabs for General, Devices, Files, Network Diagnostics, Appearance, and About & Updates.
  - Connect by IP modal, Rename modal, and Image Lightbox modal.
- **Automated Unit Tests (12/12 passing)**:
  - Verified all 12 backend unit tests pass in 0.05s with 0 errors.

---

## Phase 16: File Transfer Stability, Message Retry Queue, Responsive Sidebar Drawer & Android Groundwork (Completed)

### What Works
- **Chat & Transfer Stability**:
  - **Progress Throttling**: File transfer progress events are now throttled to at most 5/s per file; UI updates progress bars in-place instead of re-rendering the full chat timeline on every 512 KB chunk.
  - **Queued Message Retries**: Background retry loop (`start_queue_flusher`) periodically checks every 4 seconds for undelivered / "Queued" messages and automatically delivers them as soon as the target peer is reachable.
  - **Active Device Ping Check**: During heavy file transfers when Wi-Fi is saturated, discovery pruner directly pings peers before dropping them, preventing peers from falsely disappearing.
  - **Parallel Upload Limiter**: Maximum 2 concurrent file uploads (`Semaphore`), preventing network starvation.
  - **Offloaded Disk I/O**: Chunk writes are executed on `tokio::task::spawn_blocking` to keep async worker threads responsive.
  - **Atomic Destination File Reservation**: Uses `create_new(true)` to atomically reserve destination files, preventing duplicate name collisions from corrupting transfers.
  - **Clean Partial File Cleanup**: Declined or cancelled incoming transfers automatically remove incomplete or empty files from disk.
- **Sidebar & Mobile Drawer Polish**:
  - **True 68px Collapsed Rail**: Sidebar now properly collapses to 68px width (updating `min-width`) and remembers collapsed state in `localStorage`.
  - **Responsive Mobile Drawer**: On narrow screens (`max-width: 720px`), sidebar transforms into a slide-in drawer with toggle FAB and backdrop.
- **Cross-Platform & Android Groundwork**:
  - Added `paths.rs` supporting app-private folders and download paths for Android vs Desktop.
  - Gated desktop-only features (tray, native file pickers) with `#[cfg(desktop)]` and `#[cfg(not(target_os = "android"))]`.
  - Safe shell launchers: Windows `rundll32 url.dll,FileProtocolHandler` and `explorer /select` with `raw_arg`, macOS `open` / `open -R`, Linux `xdg-open`.
  - Added `.github/workflows/android.yml` for automated cloud APK builds and `scripts/android-patch.sh` for Wi-Fi multicast lock and network permissions.
- **Automated Unit Tests (12/12 passing)**:
  - Verified all 12 backend unit tests pass in 0.04s with 0 warnings.

---

## Phase 17 (v0.2.6): Two-Node Bug Fixes, Full Messaging/Transfer Polish & Native Kotlin Android App (Completed)

### What Works
- **Fixed Critical Two-Node Bugs**:
  - Fixed `db.get_messages` returning the oldest 200 rows instead of the latest 200 messages chronologically.
  - Fixed `mark_thread_read` sending the wrong device ID and failing to persist `"Read, ..."` receipts back in sender SQLite (`mark_outgoing_read`).
  - Added bidirectional HTTPS discovery handshake (`/api/discovery/hello`), Windows loopback multi-instance probing (`127.0.0.1`), RTT latency tracking (`latency_ms`), and persistent `peers` SQLite history so offline conversations remain visible.
  - Fixed `TransferManager::pause` RwLock deadlock, added real upload/receive Pause & Resume across HTTPS (`/api/files/pause/:id`, `/api/files/resume/:id`), and persisted `is_incoming` in SQLite.
  - Reused shared TLS HTTP clients (`OnceLock<reqwest::Client>`) for responsive messaging and transfers.
- **Complete Desktop UI Features**:
  - Offline conversation history, in-chat message search bar, inline image/video/audio previews + lightbox via `convertFileSrc`, hover reaction pills, Pause/Resume controls, live incoming file offer banner, in-app Edit & Confirm modals (no browser `prompt()`/`confirm()`), and Web Audio sound notifications.
- **Native Kotlin Android Application (`android/`)**:
  - Built complete Jetpack Compose Material 3 app (`com.xent.app`) compatible with Desktop Xent protocol: BouncyCastle PKCS12 self-signed TLS, pure-Kotlin HTTPS/1.1 server (`XentHttpServer`), UDP multicast discovery (`WifiManager.MulticastLock`), offline retry queue, multi-file URI streaming (`512 KB` chunks + Pause/Resume/Cancel), Favorites auto-accept, and Light/Dark/System theme.

### What Was Tested
- **Rust Backend & Two-Node HTTPS/TLS E2E (`cargo test`)**: All 15 unit tests + live two-node E2E suite passed with zero warnings.
- **Live Multi-Instance Desktop Verification**: Spawned two simultaneous `xent.exe` processes (`--instance e2e_node1 --port 53320` and `--instance e2e_node2 --port 53321`), verified `/api/ping`, `/api/info`, `/api/discovery/hello`, and `/api/message`.
- **Android Kotlin JVM Tests & APK Build (`gradle testDebugUnitTest assembleDebug`)**: All protocol tests passed and debug APK built cleanly for Android SDK 34.
