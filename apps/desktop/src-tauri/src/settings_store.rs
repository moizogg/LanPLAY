//! Persist video/host settings under the app config directory.

use lanplay_video::VideoSettings;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use parking_lot::Mutex;

static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
static CACHED: OnceLock<Mutex<VideoSettings>> = OnceLock::new();

fn cache() -> &'static Mutex<VideoSettings> {
    CACHED.get_or_init(|| Mutex::new(VideoSettings::default()))
}

/// Call once from Tauri setup with the resolved config file path.
pub fn init(path: PathBuf) {
    let _ = CONFIG_PATH.set(path.clone());
    let loaded = load_from_disk(&path).unwrap_or_default().sanitize();
    *cache().lock() = loaded;
}

fn config_path() -> PathBuf {
    CONFIG_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("lanplay-video-settings.json"))
}

fn load_from_disk(path: &PathBuf) -> Option<VideoSettings> {
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_to_disk(path: &PathBuf, settings: &VideoSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| format!("write settings: {e}"))
}

pub fn get() -> VideoSettings {
    cache().lock().clone()
}

pub fn set(settings: VideoSettings) -> Result<VideoSettings, String> {
    let cleaned = settings.sanitize();
    save_to_disk(&config_path(), &cleaned)?;
    *cache().lock() = cleaned.clone();
    Ok(cleaned)
}
