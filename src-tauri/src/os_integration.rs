use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn token_file_path() -> PathBuf {
    crate::paths::data_dir().join("local_ipc.token")
}

pub fn load_or_create_ipc_token() -> String {
    let p = token_file_path();
    if let Ok(existing) = std::fs::read_to_string(&p) {
        let t = existing.trim().to_string();
        if t.len() >= 16 {
            return t;
        }
    }
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&p, &token);
    token
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Recursively expand any directory arguments into individual files (up to a safe cap).
pub fn expand_file_paths(raw: Vec<String>) -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>, depth: u8) {
        if depth > 6 || out.len() >= 500 {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if out.len() >= 500 {
                    break;
                }
                let p = entry.path();
                if p.is_file() {
                    out.push(p.to_string_lossy().to_string());
                } else if p.is_dir() {
                    walk(&p, out, depth + 1);
                }
            }
        }
    }

    let mut out = Vec::new();
    for item in raw {
        let p = PathBuf::from(&item);
        if p.is_file() {
            out.push(item);
        } else if p.is_dir() {
            walk(&p, &mut out, 0);
        }
    }
    out
}

/// If a default primary Xent instance is already running, forward CLI file paths and focus it.
pub fn try_handoff_to_running_instance(port: u16, paths: &[String]) -> bool {
    let token = match std::fs::read_to_string(token_file_path()) {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => return false,
    };

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    rt.block_on(async move {
        let client = match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_millis(1200))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };

        let url = format!("https://127.0.0.1:{}/api/local/open-send-files", port);
        let body = serde_json::json!({ "paths": paths });
        match client
            .post(&url)
            .header("X-Xent-Local-Token", token)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    })
}

/// Register or unregister Xent in Windows HKCU Startup (`Run`) without showing a console window.
pub fn sync_startup_registration(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return,
        };
        let exe_str = exe.to_string_lossy().to_string();
        if enabled {
            let cmd_val = format!("\"{}\"", exe_str);
            let _ = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Xent",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &cmd_val,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        } else {
            let _ = std::process::Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "Xent",
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        }
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Some(cfg) = dirs::config_dir() {
            let autostart = cfg.join("autostart");
            let desktop_file = autostart.join("xent.desktop");
            if enabled {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::fs::create_dir_all(&autostart);
                    let content = format!(
                        "[Desktop Entry]\nType=Application\nName=Xent\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n",
                        exe.to_string_lossy()
                    );
                    let _ = std::fs::write(&desktop_file, content);
                }
            } else {
                let _ = std::fs::remove_file(&desktop_file);
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "android"))]
    {
        let _ = enabled;
    }
}

/// Create or remove Windows Explorer right-click "Send to -> Xent" shortcut (`SendTo\Xent.lnk`).
pub fn sync_send_to_shortcut(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        let appdata = match std::env::var_os("APPDATA") {
            Some(v) => PathBuf::from(v),
            None => return,
        };
        let sendto_lnk = appdata
            .join("Microsoft")
            .join("Windows")
            .join("SendTo")
            .join("Xent.lnk");

        if !enabled {
            let _ = std::fs::remove_file(&sendto_lnk);
            return;
        }

        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return,
        };
        let work_dir = exe
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Values are passed via environment variables, never string-interpolated into PowerShell source.
        let ps = r#"
$ws = New-Object -ComObject WScript.Shell
$s = $ws.CreateShortcut($env:XENT_LNK_PATH)
$s.TargetPath = $env:XENT_EXE_PATH
$s.WorkingDirectory = $env:XENT_WORK_DIR
$s.IconLocation = "$($env:XENT_EXE_PATH),0"
$s.Description = "Send files with Xent"
$s.Save()
"#;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", ps])
            .env("XENT_LNK_PATH", sendto_lnk.to_string_lossy().as_ref())
            .env("XENT_EXE_PATH", exe.to_string_lossy().as_ref())
            .env("XENT_WORK_DIR", &work_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
    }
}

/// Fire a native OS desktop notification asynchronously without blocking request handlers.
pub fn send_os_notification(title: String, body: String) {
    #[cfg(target_os = "windows")]
    {
        let clean_title: String = title.chars().take(100).collect();
        let clean_body: String = body.chars().take(240).collect();
        let ps = r#"
try {
  [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
  [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
  $t = [System.Security.SecurityElement]::Escape($env:XENT_NTITLE)
  $b = [System.Security.SecurityElement]::Escape($env:XENT_NBODY)
  $xmlStr = "<toast><visual><binding template='ToastGeneric'><text>$t</text><text>$b</text></binding></visual></toast>"
  $doc = New-Object Windows.Data.Xml.Dom.XmlDocument
  $doc.LoadXml($xmlStr)
  $toast = [Windows.UI.Notifications.ToastNotification]::new($doc)
  $appId = '{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe'
  [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($appId).Show($toast)
} catch {}
"#;
        std::thread::spawn(move || {
            let _ = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", ps])
                .env("XENT_NTITLE", clean_title)
                .env("XENT_NBODY", clean_body)
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        });
    }

    #[cfg(target_os = "macos")]
    {
        let clean_title: String = title.chars().take(100).collect();
        let clean_body: String = body.chars().take(240).collect();
        std::thread::spawn(move || {
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                clean_body.replace('\"', "'"),
                clean_title.replace('\"', "'")
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output();
        });
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        let clean_title: String = title.chars().take(100).collect();
        let clean_body: String = body.chars().take(240).collect();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("notify-send")
                .args([&clean_title, &clean_body])
                .output();
        });
    }

    #[cfg(target_os = "android")]
    {
        let _ = (title, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_verifies_tokens() {
        assert!(constant_time_eq("abc123", "abc123"));
        assert!(!constant_time_eq("abc123", "abc124"));
        assert!(!constant_time_eq("abc", "abcd"));
    }

    #[test]
    fn expand_file_paths_walks_nested_directories() {
        let root = std::env::temp_dir().join(format!("xent_expand_{}", uuid::Uuid::new_v4()));
        let nested = root.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        let f1 = root.join("a.txt");
        let f2 = nested.join("b.txt");
        std::fs::write(&f1, b"1").unwrap();
        std::fs::write(&f2, b"2").unwrap();

        let expanded = expand_file_paths(vec![root.to_string_lossy().to_string()]);
        assert_eq!(expanded.len(), 2);
        assert!(expanded.iter().any(|p| p.ends_with("a.txt")));
        assert!(expanded.iter().any(|p| p.ends_with("b.txt")));

        let _ = std::fs::remove_dir_all(&root);
    }
}
