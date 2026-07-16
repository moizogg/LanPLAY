//! Wire Tauri resource paths into the controllers crate and expose install UI.

use lanplay_controllers::{
    bundle_status, configure_vigem_search_paths, install_bundled_driver, probe_vigem,
    VigemBundleStatus,
};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};

/// Call once when the app starts so DLL/setup resolve from the install folder.
pub fn init_paths<R: Runtime>(app: &AppHandle<R>) {
    let mut roots: Vec<PathBuf> = Vec::new();

    // Packaged resources: resources/vigem/*
    if let Ok(dir) = app.path().resource_dir() {
        roots.push(dir.join("vigem"));
        roots.push(dir.clone());
    }

    // Next to the executable (portable / NSIS layout)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.join("resources").join("vigem"));
            roots.push(parent.join("vigem"));
            roots.push(parent.to_path_buf());
        }
    }

    // Dev: src-tauri/resources/vigem
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join("resources").join("vigem"));
        roots.push(cwd.join("src-tauri").join("resources").join("vigem"));
        roots.push(
            cwd.join("apps")
                .join("desktop")
                .join("src-tauri")
                .join("resources")
                .join("vigem"),
        );
    }

    configure_vigem_search_paths(roots);
}

pub fn status() -> VigemBundleStatus {
    let ready = probe_vigem().available;
    bundle_status(ready)
}

pub fn install_driver() -> Result<String, String> {
    install_bundled_driver()
}
