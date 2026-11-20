//! Where Xent keeps its data. `dirs::config_dir()` / `dirs::download_dir()` return
//! nothing on Android, so the app-private folders are used there.
use std::path::PathBuf;

/// Folder for settings and the chat database.
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        // Private app folder; no storage permission needed. Matches the app identifier.
        return PathBuf::from("/data/data/com.xent.app/files/Xent");
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Xent")
    }
}

/// Default folder for received files.
pub fn default_download_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        // App-specific external storage: writable without any permission prompt.
        return PathBuf::from("/storage/emulated/0/Android/data/com.xent.app/files/Download");
    }
    #[cfg(not(target_os = "android"))]
    {
        dirs::download_dir().unwrap_or_else(|| PathBuf::from("."))
    }
}

/// "mobile" on Android, "desktop" elsewhere (shown to other devices).
pub fn device_type() -> &'static str {
    if cfg!(target_os = "android") { "mobile" } else { "desktop" }
}
