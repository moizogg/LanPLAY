//! Resolve bundled ViGEm DLL / driver installer locations.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

static SEARCH_ROOTS: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

/// Tell the controllers crate where LANPlay ships ViGEm files
/// (resource dir, next to exe, etc.). Call once at app startup.
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

    // Always also try next to the running executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            roots.push(dir.join("vigem"));
            roots.push(dir.join("resources").join("vigem"));
        }
    }

    // CWD fallbacks (dev).
    roots.push(PathBuf::from("resources/vigem"));
    roots.push(PathBuf::from("vigem"));

    roots
}

/// Candidate full paths for `ViGEmClient.dll` (first existing wins at load time).
pub fn vigem_client_dll_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in search_roots() {
        out.push(root.join("ViGEmClient.dll"));
        out.push(root.join("vigem").join("ViGEmClient.dll"));
    }
    // Bare name → system PATH / DLL search order
    out.push(PathBuf::from("ViGEmClient.dll"));
    out
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
        // Any setup-looking file in the folder
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

/// Whether the client DLL is on disk somewhere we know about.
pub fn client_dll_present() -> bool {
    vigem_client_dll_candidates()
        .into_iter()
        .any(|p| p.as_os_str() != "ViGEmClient.dll" && p.is_file())
}

/// Status of what we shipped vs what Windows still needs.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VigemBundleStatus {
    pub client_dll_found: bool,
    pub client_dll_path: Option<String>,
    pub driver_setup_found: bool,
    pub driver_setup_path: Option<String>,
    pub driver_ready: bool,
    pub detail: String,
}

pub fn bundle_status(driver_ready: bool) -> VigemBundleStatus {
    let dll = vigem_client_dll_candidates().into_iter().find(|p| p.is_file());
    let setup = bundled_driver_setup();

    let detail: String = if driver_ready {
        "Virtual gamepad ready.".to_string()
    } else if setup.is_some() {
        "Gamepad driver not installed yet. Click “Install gamepad support” — one-time Windows UAC (installer is built into LANPlay)."
            .to_string()
    } else if dll.is_some() {
        "ViGEmClient.dll found, but the driver installer is missing. Re-download the full lanplay-portable folder from Actions (not just lanplay.exe)."
            .to_string()
    } else {
        "ViGEm files not found next to the app. Unzip the full lanplay-windows artifact and run lanplay.exe from that folder (keep the vigem\\ subfolder)."
            .to_string()
    };

    VigemBundleStatus {
        client_dll_found: dll.is_some(),
        client_dll_path: dll.as_ref().map(|p| p.display().to_string()),
        driver_setup_found: setup.is_some(),
        driver_setup_path: setup.as_ref().map(|p| p.display().to_string()),
        driver_ready,
        detail,
    }
}

/// Launch the bundled ViGEmBus installer elevated. One-time; user may see UAC.
pub fn install_bundled_driver() -> Result<String, String> {
    let setup = bundled_driver_setup().ok_or_else(|| {
        "Bundled ViGEmBus installer not found. This build was packaged without redist files."
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

    // Prefer ShellExecute "runas" via PowerShell so UAC elevation works for non-admin shells.
    let is_msi = setup
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"));

    let ps = if is_msi {
        // Silent-ish MSI with elevation
        format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i \"{path_str}\" /qn /norestart' -Verb RunAs -Wait"
        )
    } else {
        // Official ViGEm setup supports /qn for silent (may still show UAC)
        format!(
            "Start-Process -FilePath '{path_str}' -ArgumentList '/qn' -Verb RunAs -Wait"
        )
    };

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Failed to launch installer: {e}"))?;

    if status.success() {
        Ok(
            "Driver installer finished. If Windows asked to reboot, do that, then click Start Host again."
                .into(),
        )
    } else {
        // Non-zero can mean user cancelled UAC — still useful message
        Err(format!(
            "Installer exited with code {:?}. If you cancelled UAC, try again and accept the prompt.",
            status.code()
        ))
    }
}
