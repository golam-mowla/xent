# Changes (this update)

## Fixed: chat froze / messages did not send during and after file transfers
- Progress events were sent for every 512 KB chunk and the UI rebuilt the whole chat each time. Events are now throttled (5/s per file) and progress bars update in place.
- Messages that could not be delivered stayed "Queued" forever. A background loop now retries them every 4 s as soon as the device is reachable.
- During a big transfer the Wi-Fi drops discovery packets and the other device disappeared from the list, so nothing could be sent. Devices are now checked directly before being removed.
- Only 2 files upload at the same time; the rest wait. Disk reads/writes no longer block the network threads.
- Two files with the same name sent together wrote into the same file (corrupted both). The destination is now reserved atomically.
- A late "cancel" can no longer turn a completed transfer into a cancelled one; declined/cancelled incoming files no longer leave empty files behind.

## Fixed: sidebar
- Collapsing only shrank the contents, the 260px width stayed (min-width). It now collapses to a real 68px icon rail, and the state is remembered.

## Fixed: other
- Duplicate file cards in a chat (history + live copy). Duplicate message bubbles after a retry.
- "Open" / "Folder" buttons failed for file names with special characters (& ^ %). Open/Folder now also work on macOS/Linux.
- Failed transfers were shown as "Cancelled" in history, and had no Retry in the Transfers list.
- Sending with no selected device did nothing silently; now shows a message.

## Android groundwork
- Cargo: cdylib target, desktop-only code (tray, file dialog) gated, Android data/download folders, device type "mobile".
- Narrow-screen drawer sidebar.
- scripts/android-patch.sh (permissions + Wi-Fi multicast lock) and .github/workflows/android.yml (builds the APK in the cloud).

# Changes (this update)

## Fixed: laptop -> desktop file transfer
- Upload now starts from the Rust backend when the receiver accepts (it used to depend on the sender's chat window being open).
- Chunk errors returned HTTP 200 (sender thought data was saved). Now real error codes.
- Receiver replies to the address the request came from (no dependence on discovery list). Failed replies are reported, not ignored.
- All send errors now show a toast; failed/declined/cancelled sends have "Try again".
- Download folder is read live (was a stale snapshot). Incoming completed files are saved to history.
- Security: file names sanitised (path traversal), chunks refused until accepted.

## New
- Device nicknames (local only), favourites (pinned, auto-accept, no permission), block list.
- Settings > Devices tab, "Open Downloads Folder", clear conversation, offer notification for closed chats.
- socket2 "all" feature so Linux/macOS builds compile.

Note: README.md and PROGRESS.md in the uploaded zip contained only null bytes; restore them from git.
