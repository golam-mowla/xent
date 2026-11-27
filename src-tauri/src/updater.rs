use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const REPO_OWNER: &str = "golam-mowla";
const REPO_NAME: &str = "xent";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub release_name: String,
    pub release_notes: String,
    pub published_at: String,
    pub release_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub status: String, // "downloading", "completed", "error"
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
    assets: Vec<GitHubAsset>,
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        let cleaned = v.trim().trim_start_matches('v').trim_start_matches('V');
        cleaned
            .split('.')
            .filter_map(|s| {
                s.split(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|num| num.parse::<u32>().ok())
            })
            .collect()
    };

    let l_parts = parse(latest);
    let c_parts = parse(current);

    let max_len = l_parts.len().max(c_parts.len());
    for i in 0..max_len {
        let l = l_parts.get(i).copied().unwrap_or(0);
        let c = c_parts.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }
    false
}

fn matches_platform_installer(name: &str) -> bool {
    let n = name.to_lowercase();
    #[cfg(target_os = "windows")]
    {
        n.ends_with(".exe")
    }
    #[cfg(target_os = "macos")]
    {
        n.ends_with(".dmg") || n.ends_with(".app.tar.gz")
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        n.ends_with(".appimage") || n.ends_with(".deb")
    }
}

fn parse_tag_from_release_url(url: &str) -> Option<String> {
    let marker = "/releases/tag/";
    let idx = url.find(marker)?;
    let tail = &url[idx + marker.len()..];
    let tag = tail.split(&['?', '#', '/'][..]).next()?.trim();
    if tag.is_empty() {
        None
    } else {
        Some(tag.to_string())
    }
}

fn parse_asset_from_expanded_html(html: &str, tag: &str) -> Option<(String, String)> {
    let prefix = format!("/{}/{}/releases/download/{}/", REPO_OWNER, REPO_NAME, tag);
    let mut search_from = 0;
    while let Some(pos) = html[search_from..].find(&prefix) {
        let abs_start = search_from + pos;
        let tail = &html[abs_start..];
        let end = tail.find(&['"', '\'', '<', ' ', '\n'][..]).unwrap_or(tail.len());
        let rel_url = &tail[..end];
        let file_name = rel_url.rsplit('/').next().unwrap_or("").to_string();
        if matches_platform_installer(&file_name) {
            return Some((format!("https://github.com{}", rel_url), file_name));
        }
        search_from = abs_start + prefix.len();
    }
    None
}

async fn check_via_web_redirect(
    client: &reqwest::Client,
    current_version: &str,
) -> Result<UpdateInfo, String> {
    let latest_web_url = format!("https://github.com/{}/{}/releases/latest", REPO_OWNER, REPO_NAME);
    let resp = client
        .get(&latest_web_url)
        .header("Accept", "text/html")
        .send()
        .await
        .map_err(|e| format!("Failed to reach GitHub releases page: {}", e))?;

    let final_url = resp.url().to_string();
    let tag = parse_tag_from_release_url(&final_url)
        .ok_or_else(|| "No published GitHub release tag found yet".to_string())?;
    let latest_version = tag.trim_start_matches('v').trim_start_matches('V').to_string();
    let has_update = is_newer_version(&latest_version, current_version);

    let expanded_url = format!(
        "https://github.com/{}/{}/releases/expanded_assets/{}",
        REPO_OWNER, REPO_NAME, tag
    );
    let mut download_url = None;
    let mut asset_name = None;
    if let Ok(assets_resp) = client.get(&expanded_url).header("Accept", "text/html").send().await {
        if assets_resp.status().is_success() {
            if let Ok(html) = assets_resp.text().await {
                if let Some((url, name)) = parse_asset_from_expanded_html(&html, &tag) {
                    download_url = Some(url);
                    asset_name = Some(name);
                }
            }
        }
    }

    Ok(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version,
        has_update,
        release_name: tag.clone(),
        release_notes: String::new(),
        published_at: String::new(),
        release_url: final_url,
        download_url,
        asset_name,
        asset_size: None,
    })
}

#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    );

    let client = reqwest::Client::builder()
        .user_agent(format!("Xent-App/{}", current_version))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => {
            if let Ok(release) = res.json::<GitHubRelease>().await {
                let latest_version = release
                    .tag_name
                    .trim_start_matches('v')
                    .trim_start_matches('V')
                    .to_string();
                let has_update = is_newer_version(&latest_version, &current_version);
                let installer_asset = release
                    .assets
                    .into_iter()
                    .find(|a| matches_platform_installer(&a.name));
                let (download_url, asset_name, asset_size) = match installer_asset {
                    Some(a) => (Some(a.browser_download_url), Some(a.name), Some(a.size)),
                    None => (None, None, None),
                };
                return Ok(UpdateInfo {
                    current_version,
                    latest_version,
                    has_update,
                    release_name: release.name.unwrap_or(release.tag_name),
                    release_notes: release.body.unwrap_or_default(),
                    published_at: release.published_at.unwrap_or_default(),
                    release_url: release.html_url,
                    download_url,
                    asset_name,
                    asset_size,
                });
            }
        }
        _ => {}
    }

    // Fallback when GitHub API hits 403/429 unauthenticated rate limits
    check_via_web_redirect(&client, &current_version).await
}

#[tauri::command]
pub async fn start_update_download(
    app: AppHandle,
    download_url: String,
    expected_size: Option<u64>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(format!("Xent-App/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let res = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to start update download: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("Download failed with status: {}", res.status()));
    }

    let total_bytes = res.content_length().or(expected_size).unwrap_or(0);
    let temp_dir = std::env::temp_dir();
    let dest_path = temp_dir.join("Xent_update_setup.exe");

    let mut file = File::create(&dest_path)
        .map_err(|e| format!("Failed to create temporary installer file: {}", e))?;

    let mut stream = res.bytes_stream();
    let mut downloaded_bytes: u64 = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Error during download: {}", e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("Error writing update file: {}", e))?;
        downloaded_bytes += chunk.len() as u64;

        let percent = if total_bytes > 0 {
            ((downloaded_bytes as f64 / total_bytes as f64) * 100.0).min(100.0)
        } else {
            0.0
        };

        let _ = app.emit(
            "update://progress",
            DownloadProgress {
                downloaded_bytes,
                total_bytes,
                percent,
                status: "downloading".to_string(),
                error: None,
            },
        );
    }

    file.flush()
        .map_err(|e| format!("Failed to flush installer file: {}", e))?;

    let _ = app.emit(
        "update://progress",
        DownloadProgress {
            downloaded_bytes,
            total_bytes,
            percent: 100.0,
            status: "completed".to_string(),
            error: None,
        },
    );

    Ok(dest_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let dest_path = temp_dir.join("Xent_update_setup.exe");
    if !dest_path.exists() {
        return Err("Update installer not found. Please download again.".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(&dest_path)
            .spawn()
            .map_err(|e| format!("Failed to launch installer: {}", e))?;
        app.exit(0);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Automatic installation is currently supported on Windows. Please run the downloaded installer manually.".to_string())
    }
}

#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.2.3", "0.2.2"));
        assert!(is_newer_version("v0.2.3", "0.2.2"));
        assert!(is_newer_version("0.3.0", "0.2.2"));
        assert!(is_newer_version("1.0.0", "0.2.2"));
        assert!(is_newer_version("0.2.2.1", "0.2.2"));
        assert!(!is_newer_version("0.2.2", "0.2.2"));
        assert!(!is_newer_version("v0.2.2", "v0.2.2"));
        assert!(!is_newer_version("0.2.1", "0.2.2"));
        assert!(!is_newer_version("0.1.9", "0.2.2"));
    }

    #[test]
    fn test_web_fallback_parsers() {
        assert_eq!(
            parse_tag_from_release_url("https://github.com/golam-mowla/xent/releases/tag/v0.2.8"),
            Some("v0.2.8".to_string())
        );
        let sample = r#"
          <a href="/golam-mowla/xent/releases/download/v0.2.8/Xent-Android-v0.2.8.apk">apk</a>
          <a href="/golam-mowla/xent/releases/download/v0.2.8/Xent_0.2.8_x64-setup.exe">exe</a>
        "#;
        let parsed = parse_asset_from_expanded_html(sample, "v0.2.8");
        #[cfg(target_os = "windows")]
        assert_eq!(
            parsed,
            Some((
                "https://github.com/golam-mowla/xent/releases/download/v0.2.8/Xent_0.2.8_x64-setup.exe".to_string(),
                "Xent_0.2.8_x64-setup.exe".to_string()
            ))
        );
    }
}
