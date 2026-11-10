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
