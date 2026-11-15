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
