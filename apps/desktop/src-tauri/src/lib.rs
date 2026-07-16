mod session;
mod tailscale;
mod vigem_setup;

use lanplay_controllers::VigemBundleStatus;
use lanplay_protocol::PROTOCOL_VERSION;
use lanplay_shared::{ClientStatus, ControllerStats, HostStatus, TailscaleInfo};
use session::SessionManager;
use tauri::State;

#[tauri::command]
fn get_app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "LANPlay",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "phase": 2,
    })
}

#[tauri::command]
fn get_tailscale_info(fresh: Option<bool>) -> TailscaleInfo {
    if fresh.unwrap_or(false) {
        tailscale::detect_tailscale_fresh()
    } else {
        tailscale::detect_tailscale()
    }
}

#[tauri::command]
fn get_host_status(session: State<'_, SessionManager>) -> HostStatus {
    session.host_status()
}

#[tauri::command]
fn get_client_status(session: State<'_, SessionManager>) -> ClientStatus {
    session.client_status()
}

#[tauri::command]
fn get_controller_stats(session: State<'_, SessionManager>) -> ControllerStats {
    session.controller_stats()
}

#[tauri::command]
fn get_vigem_bundle_status() -> VigemBundleStatus {
    vigem_setup::status()
}

#[tauri::command]
fn install_vigem_driver() -> Result<String, String> {
    let msg = vigem_setup::install_driver()?;
    Ok(msg)
}

#[tauri::command]
fn start_host(session: State<'_, SessionManager>) -> Result<HostStatus, String> {
    session.start_host()
}

#[tauri::command]
fn stop_host(session: State<'_, SessionManager>) -> Result<HostStatus, String> {
    session.stop_host()
}

#[tauri::command]
fn set_allow_remote_input(session: State<'_, SessionManager>, allow: bool) -> HostStatus {
    session.set_allow_remote_input(allow)
}

#[tauri::command]
fn connect_client(
    session: State<'_, SessionManager>,
    host_ip: String,
    control_port: u16,
    media_port: u16,
) -> Result<ClientStatus, String> {
    session.connect_client(host_ip, control_port, media_port)
}

#[tauri::command]
fn disconnect_client(session: State<'_, SessionManager>) -> Result<ClientStatus, String> {
    session.disconnect_client()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(SessionManager::new())
        .setup(|app| {
            vigem_setup::init_paths(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            get_tailscale_info,
            get_host_status,
            get_client_status,
            get_controller_stats,
            get_vigem_bundle_status,
            install_vigem_driver,
            start_host,
            stop_host,
            set_allow_remote_input,
            connect_client,
            disconnect_client,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LANPlay");
}
