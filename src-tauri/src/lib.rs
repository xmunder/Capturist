use std::sync::Mutex;

mod capture;
mod commands;
mod encoder;
mod region;
mod shortcuts;

use capture::manager::CaptureManager;
use shortcuts::GlobalShortcutManager;
use tauri::Manager;

pub struct AppState {
    pub capture: Mutex<CaptureManager>,
    pub global_shortcuts: Mutex<Option<GlobalShortcutManager>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            capture: Mutex::new(CaptureManager::new()),
            global_shortcuts: Mutex::new(None),
        }
    }

    pub fn set_global_shortcuts(&self, manager: GlobalShortcutManager) -> Result<(), String> {
        let mut guard = self
            .global_shortcuts
            .lock()
            .map_err(|_| "No se pudo guardar el gestor de atajos globales".to_string())?;
        *guard = Some(manager);
        Ok(())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let manager = GlobalShortcutManager::new(app.handle().clone()).map_err(|err| {
                std::io::Error::other(format!("No se pudo iniciar atajos globales: {err}"))
            })?;

            app.state::<AppState>()
                .set_global_shortcuts(manager)
                .map_err(std::io::Error::other)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::is_capture_supported,
            commands::get_targets,
            commands::get_audio_input_devices,
            commands::get_recording_audio_status,
            commands::set_global_shortcuts,
            commands::start_recording,
            commands::update_recording_audio_capture,
            commands::pause_recording,
            commands::resume_recording,
            commands::stop_recording,
            commands::cancel_recording,
            commands::get_recording_status,
            commands::select_region_native,
        ])
        .run(tauri::generate_context!())
        .expect("Error al iniciar la aplicación Tauri");
}
