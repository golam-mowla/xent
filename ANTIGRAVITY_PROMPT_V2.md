# Xent v2: bug fixes + proper messenger and file transfer (Antigravity task)

Context: Phases 1 to 9 were reported "completed", but on a real run the app shows NO nearby devices and the attach button does nothing. Root cause found: `src-tauri/tauri.conf.json` did not set `app.withGlobalTauri: true`, so `window.__TAURI__` was undefined and every `invoke`/`listen` in `ui/index.html` was silently skipped (the code is guarded by `if (window.__TAURI__ ...)`). That flag, an adapter-aware discovery rewrite (multicast + broadcast on every adapter), a toast on attach, and a longer peer timeout (15 s) are ALREADY applied in this repo, but they have NOT been compiled or run. Step 0 is to compile and prove them.

Rules that never change: fast and lightweight (idle RAM < 100 MB, cold start < 2 s), vanilla HTML/CSS/JS, no CDN, as few new crates as possible, keep LocalSend-compatible discovery on `224.0.0.167:53317`, keep the glass UI from `ui-reference/glass.html`, keep sender/receiver attribution on every bubble and file card. Never mark a task done on the basis of reading code: run the real app and test through the UI.

## Step 0: compile and prove the hotfix (do first, report in PROGRESS.md)

1. `cargo check` then `cargo tauri dev`. Fix every error/warning in `discovery.rs` (new `local_v4_addrs`, `build_sender`, `send_everywhere`).
2. Open the WebView devtools console. Confirm `window.__TAURI__` exists and there are no errors on load.
3. Run two instances (`--instance node2 --port 53325`). Confirm each shows the other in Nearby and the header label changes to "Nearby, 1 online".
4. Select the peer, click the paperclip, pick a file, accept on the other side, confirm it lands in Downloads.
5. Test on two REAL machines on the same Wi-Fi (not only two instances on one PC). Record whether discovery works. If not, continue to P0.

## P0: make discovery reliable on real networks

- **Subnet scan fallback (LocalSend "legacy" method).** If no peer has been seen within 5 s, and then every 15 s, scan every IPv4 address of each adapter's /24 for `https://<ip>:<port>/api/info` (port = our port and 53317), concurrency about 64, timeout 400 ms, `danger_accept_invalid_certs(true)`. Valid responses become peers (use `DeviceInfoResponse`). Stop scanning when peers are found; keep it off the hot path.
- **Add device by IP.** In the Nearby panel add an "Add by IP" button (input `192.168.x.x[:port]`); it calls `/api/info`, adds the peer, remembers it in `settings.json` (`manual_peers`) and re-probes them every 10 s.
- **Network diagnostics panel** (Settings > Network): list local IPv4 addresses and adapter names, our HTTPS port, whether the UDP listener is bound, timestamp of the last discovery packet received, number of peers, and a "Copy firewall command" button producing
  `netsh advfirewall firewall add rule name="Xent" dir=in action=allow program="<full path to xent.exe>" enable=yes profile=any` plus `... protocol=UDP localport=53317`.
- **First-run hint:** if after 15 s no peers are found, show a dismissible glass banner explaining: same Wi-Fi, Windows network profile must be Private, allow Xent in the firewall, router "AP/client isolation" or guest Wi-Fi blocks devices.
- Re-announce immediately when the network changes (poll `local_v4_addrs()` every 5 s; if the set changed, rejoin multicast groups and announce).

## P0: attach and file sending

- Paperclip works with no preselected peer if exactly one peer exists (auto-select it); otherwise toast "Select a device first".
- **Multiple files** at once (`rfd` `pick_files`), one card per file, transfers can run concurrently.
- **Drag and drop** onto the chat panel (Tauri drag-drop event) with a glass drop overlay "Drop to send to <name>".
- **Paste images** from the clipboard into the compose box and send as files.
- File cards after completion: **Open**, **Show in folder** buttons, image/video thumbnail preview for images.
- File cards must persist in chat history after restart (store transfer rows in the `transfers` table with `thread_peer_id` and render them inline in the thread by timestamp).
- Choose download folder in Settings (native folder picker). Optional **auto-accept from trusted devices** (per-peer toggle).
- Transfer robustness: resume interrupted transfers from the last confirmed offset, per-chunk SHA-256 check or whole-file hash at the end (sha2 already in deps), clear error messages, retry button on failed cards.
- Overall transfer speed uses a moving average; ETA never flickers.

## P1: messaging features (Teams-style)

- **Message states:** Sending, Delivered, Read (blue ticks) with read receipts when the thread is opened and window focused.
- **Typing indicator** ("Radiant Stag is typing...") sent over the existing WebSocket, throttled to once per 2 s.
- **Reply/quote**, **edit** (within 15 min, shows "edited") and **delete for everyone** (tombstone), **emoji reactions** (a small fixed set), **copy** and **forward**.
- **Clickable links**, multi-line messages (Shift+Enter), emoji picker (small, local, no library), `@mention` not needed for 1-to-1.
- **Offline queue:** if the peer is offline, keep the message as "Queued" and deliver automatically when the peer reappears.
- **Search:** global search over messages (SQLite FTS5 if available in bundled build, else LIKE) with a search box in the list panel and in-thread search (Ctrl+F).
- **Unread:** badge per peer, bold name, tray tooltip/total count, "jump to unread" marker line in the thread.
- **Native notifications** (plugin already present) for new messages and incoming file requests when the window is not focused; mute per peer and global "Do not disturb" tied to the Busy status.
- **Groups / channels:** create a group (name, members chosen from Nearby), messages fan out to every member over HTTPS, group files are sent per member, group id + member list stored in SQLite, shown in the list with a stacked avatar. Keep it simple (no admin roles).
- **Presence upgrades:** auto Away after 10 minutes idle (and back on activity), "Last seen" for peers that went offline (kept in the list as greyed entries from SQLite `known_peers`).
- **Pairing/trust:** show a short fingerprint code per peer; "Trust this device" stores the fingerprint and warns if it changes. Optional PIN to accept first contact.

## P1: settings and app quality

- Settings tabs: General (alias, theme, device type), Notifications, Files (download dir, auto-accept), Network (diagnostics), About (version, log folder).
- Start with Windows (user toggle), minimize to tray on close (already), single-instance guard (second launch focuses the first window), launch minimized option.
- Logging to `%APPDATA%\Xent\logs\xent.log` with rotation (tauri-plugin-log is already a dependency; actually enable it).
- Keyboard shortcuts: Ctrl+K quick switch peer, Ctrl+F search, Esc closes modals, Ctrl+Shift+A attach.
- Accessibility: focus rings, aria-labels on icon buttons, minimum contrast in light and dark glass modes.
- Empty states and error toasts everywhere (no silent `console.error` only).
- Make `window.__TAURI__` failures visible: if it is missing show a red banner "Backend not connected" instead of silently doing nothing.

## P2: polish

- Chat message grouping by day with date separators, sticky header, smooth scroll to bottom only when already at bottom.
- Image paste/preview lightbox, video/audio inline for received media.
- Export chat as text/JSON, clear history per peer.
- NSIS installer: create Start Menu shortcut, optional firewall rule step (document the admin requirement), uninstall removes `%APPDATA%\Xent` only if the user ticks a box.
- GitHub Actions release workflow already exists; make it also run `cargo check` and `cargo test`.
- Add unit tests for packet parsing (old packets without `status` still parse), settings migration, and the transfer chunk assembler.

## Performance guard (re-measure after every phase)

Idle RAM < 100 MB, cold start < 2 s, scan fallback must not run when peers are present, no polling timers faster than 1 s in JS, DOM capped to 200 messages with "Load earlier". If a feature breaks a target, simplify the feature.

## Deliverables

- Everything committed in small commits, `PROGRESS.md` with Phase 10 (Step 0 + P0), Phase 11 (P1), Phase 12 (P2); each phase lists what was tested ON REAL UI and on two physical PCs, with measured RAM/startup.
- README feature list and troubleshooting section (firewall, Private network, router isolation).
