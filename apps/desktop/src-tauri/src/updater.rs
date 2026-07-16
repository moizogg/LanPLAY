//! In-app update from the rolling GitHub Release tag `nightly`.
//!
//! CI publishes `lanplay-windows-nightly.zip` on every green `main` build.
//! User clicks Update once — no more manual Actions artifact downloads.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{copy, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "moizogg/LanPLAY";
const NIGHTLY_TAG: &str = "nightly";
const USER_AGENT: &str = "LANPlay-Updater";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_sha: String,
    pub current_version: String,
    pub latest_sha: Option<String>,
    pub latest_name: Option<String>,
    pub download_url: Option<String>,
    pub update_available: bool,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    name: Option<String>,
    body: Option<String>,
    tag_name: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn current_git_sha() -> String {
    option_env!("LANPLAY_GIT_SHA")
        .unwrap_or("dev")
        .chars()
        .take(12)
        .collect()
}

fn http_get_json(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("network: {e}"))?;
    resp.into_string()
        .map_err(|e| format!("read body: {e}"))
}

fn parse_sha_from_release(rel: &GhRelease) -> Option<String> {
    // Prefer body line "Commit: <sha>"
    if let Some(body) = &rel.body {
        for line in body.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("Commit:") {
                let s = rest.trim();
                if s.len() >= 7 {
                    return Some(s.chars().take(12).collect());
                }
            }
            if let Some(rest) = t.strip_prefix("commit:") {
                let s = rest.trim();
                if s.len() >= 7 {
                    return Some(s.chars().take(12).collect());
                }
            }
        }
    }
    // Fallback: asset name lanplay-windows-<sha>.zip
    for a in &rel.assets {
        if let Some(s) = a.name.strip_prefix("lanplay-windows-") {
            if let Some(s) = s.strip_suffix(".zip") {
                if s.len() >= 7 && s != "nightly" {
                    return Some(s.chars().take(12).collect());
                }
            }
        }
    }
    None
}

fn pick_zip_asset(rel: &GhRelease) -> Option<&GhAsset> {
    rel.assets
        .iter()
        .find(|a| a.name.ends_with(".zip") && a.name.contains("lanplay"))
        .or_else(|| rel.assets.iter().find(|a| a.name.ends_with(".zip")))
}

/// Check nightly release vs this build's git SHA.
pub fn check_for_update() -> UpdateStatus {
    let current_sha = current_git_sha();
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let url = format!("https://api.github.com/repos/{REPO}/releases/tags/{NIGHTLY_TAG}");

    match http_get_json(&url) {
        Ok(body) => match serde_json::from_str::<GhRelease>(&body) {
            Ok(rel) => {
                let latest_sha = parse_sha_from_release(&rel);
                let asset_url = pick_zip_asset(&rel).map(|a| a.browser_download_url.clone());
                let asset_mb = pick_zip_asset(&rel).map(|a| a.size / 1_000_000).unwrap_or(0);
                let latest_name = rel.name.clone().or(rel.tag_name.clone());
                let has_asset = asset_url.is_some();
                let update_available = match (&latest_sha, has_asset) {
                    (Some(latest), true) => {
                        let cur = current_sha.to_lowercase();
                        let lat = latest.to_lowercase();
                        // dev builds always offer; otherwise compare short SHAs
                        cur == "dev"
                            || cur.is_empty()
                            || !(lat.starts_with(&cur) || cur.starts_with(&lat))
                    }
                    (None, true) => current_sha == "dev",
                    _ => false,
                };

                UpdateStatus {
                    current_sha: current_sha.clone(),
                    current_version,
                    latest_sha: latest_sha.clone(),
                    latest_name,
                    download_url: asset_url,
                    update_available,
                    detail: if update_available {
                        format!(
                            "Update available: {} → {} ({} MB)",
                            current_sha,
                            latest_sha.as_deref().unwrap_or("?"),
                            asset_mb
                        )
                    } else if !has_asset {
                        "Nightly release has no zip asset yet.".into()
                    } else {
                        format!("You're on the latest nightly ({current_sha}).")
                    },
                }
            }
            Err(e) => UpdateStatus {
                current_sha,
                current_version,
                latest_sha: None,
                latest_name: None,
                download_url: None,
                update_available: false,
                detail: format!("Bad release JSON: {e}"),
            },
        },
        Err(e) => UpdateStatus {
            current_sha,
            current_version,
            latest_sha: None,
            latest_name: None,
            download_url: None,
            update_available: false,
            detail: format!("Could not check updates: {e}"),
        },
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("download: {e}"))?;
    let mut reader = resp.into_reader();
    let mut file = File::create(dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    copy(&mut reader, &mut file).map_err(|e| format!("write zip: {e}"))?;
    Ok(())
}

fn extract_zip(zip_path: &Path, out_dir: &Path) -> Result<(), String> {
    let file = File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| "bad zip path".to_string())?
            .to_owned();
        let out = out_dir.join(&name);
        if file.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut outfile = File::create(&out).map_err(|e| e.to_string())?;
            copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn find_new_exe(dir: &Path) -> Option<PathBuf> {
    // Prefer nested portable layout
    let candidates = [
        dir.join("lanplay.exe"),
        dir.join("lanplay-portable").join("lanplay.exe"),
    ];
    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    // Walk once
    fn walk(d: &Path) -> Option<PathBuf> {
        let rd = fs::read_dir(d).ok()?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case("lanplay.exe"))
                    .unwrap_or(false)
            {
                return Some(p);
            }
            if p.is_dir() {
                if let Some(f) = walk(&p) {
                    return Some(f);
                }
            }
        }
        None
    }
    walk(dir)
}

/// Download nightly zip, stage new exe, spawn replace script, return message.
/// Caller should exit the app shortly after so the file can be replaced.
pub fn apply_update() -> Result<String, String> {
    let status = check_for_update();
    let url = status
        .download_url
        .ok_or_else(|| status.detail.clone())?;

    let temp = std::env::temp_dir().join("lanplay-update");
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let zip_path = temp.join("lanplay-nightly.zip");
    let extract_dir = temp.join("extract");

    download_file(&url, &zip_path)?;
    extract_zip(&zip_path, &extract_dir)?;
    let new_exe = find_new_exe(&extract_dir).ok_or_else(|| {
        "Zip did not contain lanplay.exe — bad nightly package.".to_string()
    })?;

    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let install_dir = current
        .parent()
        .ok_or_else(|| "no install dir".to_string())?
        .to_path_buf();
    let staged = install_dir.join("lanplay_new.exe");
    fs::copy(&new_exe, &staged).map_err(|e| format!("stage new exe: {e}"))?;

    // Also refresh vigem folder if present in package
    if let Some(vigem_src) = [
        extract_dir.join("vigem"),
        extract_dir.join("lanplay-portable").join("vigem"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
    {
        let vigem_dst = install_dir.join("vigem");
        let _ = fs::create_dir_all(&vigem_dst);
        if let Ok(rd) = fs::read_dir(&vigem_src) {
            for e in rd.flatten() {
                let to = vigem_dst.join(e.file_name());
                let _ = fs::copy(e.path(), to);
            }
        }
    }

    // Write build-info next to exe if package has it
    for name in ["build-info.json", "BUILD_INFO.txt"] {
        for base in [&extract_dir, &extract_dir.join("lanplay-portable")] {
            let src = base.join(name);
            if src.is_file() {
                let _ = fs::copy(&src, install_dir.join(name));
            }
        }
    }

    let bat = install_dir.join("lanplay_apply_update.bat");
    let bat_body = format!(
        r#"@echo off
setlocal
cd /d "{dir}"
echo Updating LANPlay...
rem Wait for this process to exit so lanplay.exe is not locked
timeout /t 2 /nobreak >nul
:wait
tasklist /FI "IMAGENAME eq lanplay.exe" 2>NUL | find /I "lanplay.exe" >NUL
if not errorlevel 1 (
  timeout /t 1 /nobreak >nul
  goto wait
)
if exist "lanplay.exe.bak" del /f /q "lanplay.exe.bak"
if exist "lanplay.exe" move /y "lanplay.exe" "lanplay.exe.bak" >nul
move /y "lanplay_new.exe" "lanplay.exe" >nul
if errorlevel 1 (
  echo Update failed — restoring backup
  if exist "lanplay.exe.bak" move /y "lanplay.exe.bak" "lanplay.exe" >nul
  pause
  exit /b 1
)
start "" "lanplay.exe"
del /f /q "%~f0"
endlocal
"#,
        dir = install_dir.display()
    );
    {
        let mut f = File::create(&bat).map_err(|e| e.to_string())?;
        f.write_all(bat_body.as_bytes()).map_err(|e| e.to_string())?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .args(["/C", "start", "", &bat.display().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("spawn updater: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("sh").arg("-c").arg("true").spawn();
        return Err("In-app update is Windows-only for now.".into());
    }

    Ok(format!(
        "Update downloaded ({}). LANPlay will restart in a moment…",
        status.latest_sha.unwrap_or_else(|| "?".into())
    ))
}
