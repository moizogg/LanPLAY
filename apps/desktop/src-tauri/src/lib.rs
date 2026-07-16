mod firewall;
mod session;
mod settings_store;
mod tailscale;
mod updater;
mod vigem_setup;

use lanplay_controllers::{CaptureStatus, VigemBundleStatus};
use lanplay_protocol::PROTOCOL_VERSION;
use lanplay_shared::{ClientStatus, ControllerStats, HostStatus, TailscaleInfo};
use lanplay_video::{
    list_encoder_options, resolution_presets, CaptureSnapshot, ClientVideoSnapshot, EncoderOption,
    ResolutionPreset, VideoSettings,
};
use session::SessionManager;
use tauri::Manager;
use tauri::State;

#[tauri::command]
fn get_app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "LANPlay",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "phase": 6,
        "gitSha": updater::current_git_sha(),
    })
}

#[tauri::command]
fn check_for_update() -> updater::UpdateStatus {
    updater::check_for_update()
}

#[tauri::command]
fn apply_update() -> Result<String, String> {
    updater::apply_update()
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
fn respond_to_join(session: State<'_, SessionManager>, accept: bool) -> Result<HostStatus, String> {
    session.respond_to_join(accept)
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

#[tauri::command]
fn get_input_capture(session: State<'_, SessionManager>) -> CaptureStatus {
    session.get_input_capture()
}

#[tauri::command]
fn set_input_capture(
    session: State<'_, SessionManager>,
    active: bool,
) -> Result<CaptureStatus, String> {
    session.set_input_capture(active)
}

#[tauri::command]
fn toggle_input_capture(session: State<'_, SessionManager>) -> Result<CaptureStatus, String> {
    session.toggle_input_capture()
}

#[tauri::command]
fn get_capture_stats(session: State<'_, SessionManager>) -> CaptureSnapshot {
    session.get_capture_stats()
}

#[tauri::command]
fn get_client_video(session: State<'_, SessionManager>) -> ClientVideoSnapshot {
    session.get_client_video()
}

#[tauri::command]
fn get_video_settings(session: State<'_, SessionManager>) -> VideoSettings {
    session.get_video_settings()
}

#[tauri::command]
fn set_video_settings(
    session: State<'_, SessionManager>,
    settings: VideoSettings,
) -> Result<VideoSettings, String> {
    session.set_video_settings(settings)
}

#[tauri::command]
fn get_encoder_options() -> Vec<EncoderOption> {
    list_encoder_options()
}

#[tauri::command]
fn get_hardware_encoder_probe() -> String {
    lanplay_video::hardware_encoder_probe()
}

#[tauri::command]
fn get_resolution_presets() -> Vec<ResolutionPreset> {
    resolution_presets()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(SessionManager::new())
        .setup(|app| {
            vigem_setup::init_paths(app.handle());
            // Persist Settings under OS app config dir (like Sunshine config).
            if let Ok(dir) = app.path().app_config_dir() {
                settings_store::init(dir.join("video_settings.json"));
            } else {
                settings_store::init(std::path::PathBuf::from("video_settings.json"));
            }
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
            respond_to_join,
            set_allow_remote_input,
            connect_client,
            disconnect_client,
            get_input_capture,
            set_input_capture,
            toggle_input_capture,
            get_capture_stats,
            get_client_video,
            get_video_settings,
            set_video_settings,
            get_encoder_options,
            get_hardware_encoder_probe,
            get_resolution_presets,
            check_for_update,
            apply_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LANPlay");
}
