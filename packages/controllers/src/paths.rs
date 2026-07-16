//! Resolve bundled ViGEm **driver installer** locations.
//!
//! ViGEmClient is statically linked into the app — only the kernel driver setup
//! is an external file (optional one-click install).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

static SEARCH_ROOTS: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

/// Tell the controllers crate where LANPlay ships the ViGEmBus setup.
pub fn configure_vigem_search_paths(paths: Vec<PathBuf>) {
    if let Ok(mut g) = SEARCH_ROOTS.write() {
        *g = paths;
    }
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = SEARCH_ROOTS
        .read()
        .map(|g| g.clone())
        .unwrap_or_default();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            roots.push(dir.join("vigem"));
            roots.push(dir.join("resources").join("vigem"));
        }
    }

    roots.push(PathBuf::from("resources/vigem"));
    roots.push(PathBuf::from("vigem"));

    roots
}

/// Bundled driver setup (exe or msi), if present.
pub fn bundled_driver_setup() -> Option<PathBuf> {
    const NAMES: &[&str] = &[
        "ViGEmBus_Setup.exe",
        "ViGEmBus_Setup.msi",
        "ViGEmBusSetup.exe",
        "ViGEmBusSetup_x64.exe",
        "ViGEmBusSetup_x64.msi",
    ];

    for root in search_roots() {
        for name in NAMES {
            let p = root.join(name);
            if p.is_file() {
                return Some(p);
            }
            let p2 = root.join("vigem").join(name);
            if p2.is_file() {
                return Some(p2);
            }
        }
        if let Ok(rd) = std::fs::read_dir(&root) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if name.to_ascii_lowercase().contains("vigembus")
                    && (name.ends_with(".exe") || name.ends_with(".msi"))
                {
                    return Some(ent.path());
                }
            }
        }
    }
    None
}

/// Status of driver packaging + bus readiness.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VigemBundleStatus {
    /// Always true when built with static ViGEmClient (Windows host builds).
    pub client_dll_found: bool,
    pub client_dll_path: Option<String>,
    pub driver_setup_found: bool,
    pub driver_setup_path: Option<String>,
    pub driver_ready: bool,
    pub detail: String,
}

pub fn bundle_status(driver_ready: bool) -> VigemBundleStatus {
    let setup = bundled_driver_setup();

    // Client is linked into the binary — no external DLL.
    let client_linked = cfg!(windows);

    let detail: String = if driver_ready {
        "Virtual gamepad ready (ViGEmClient built into LANPlay).".to_string()
    } else if setup.is_some() {
        "ViGEmBus driver not installed yet. Click “Install gamepad support” — one-time Windows UAC (installer is bundled)."
            .to_string()
    } else if client_linked {
        "ViGEmClient is inside LANPlay, but the driver setup file is missing from this package. Re-download the full portable zip from Actions."
            .to_string()
    } else {
        "Gamepad virtualization is Windows-only.".to_string()
    };

    VigemBundleStatus {
        client_dll_found: client_linked,
        client_dll_path: if client_linked {
            Some("(statically linked into lanplay.exe)".into())
        } else {
            None
        },
        driver_setup_found: setup.is_some(),
        driver_setup_path: setup.as_ref().map(|p| p.display().to_string()),
        driver_ready,
        detail,
    }
}

/// Launch the bundled ViGEmBus installer elevated. One-time; user may see UAC.
pub fn install_bundled_driver() -> Result<String, String> {
    let setup = bundled_driver_setup().ok_or_else(|| {
        "Bundled ViGEmBus installer not found. Re-download the full lanplay-portable package."
            .to_string()
    })?;

    #[cfg(windows)]
    {
        install_windows(&setup)
    }
    #[cfg(not(windows))]
    {
        let _ = setup;
        Err("Driver install is only supported on Windows.".into())
    }
}

#[cfg(windows)]
fn install_windows(setup: &Path) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let path_str = setup
        .to_str()
        .ok_or_else(|| "Installer path is not valid UTF-8".to_string())?;

    let is_msi = setup
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"));

    let ps = if is_msi {
        format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i \"{path_str}\" /qn /norestart' -Verb RunAs -Wait"
        )
    } else {
        format!("Start-Process -FilePath '{path_str}' -ArgumentList '/qn' -Verb RunAs -Wait")
    };

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to launch installer: {e}"))?;

    if status.success() {
        Ok(
            "Driver installer finished. If Windows asked to reboot, do that, then Start Host again."
                .into(),
        )
    } else {
        Err(format!(
            "Installer exited with code {:?}. If you cancelled UAC, try again and accept the prompt.",
            status.code()
        ))
    }
}
