use std::sync::{Mutex, OnceLock};

use crate::encoder::config::AudioCaptureConfig;

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAudioStatusSnapshot {
    pub capture_system_audio: bool,
    pub capture_microphone_audio: bool,
    pub system_audio_device_name: Option<String>,
    pub microphone_audio_device_name: Option<String>,
}

fn live_audio_status() -> &'static Mutex<LiveAudioStatusSnapshot> {
    static LIVE_AUDIO_STATUS: OnceLock<Mutex<LiveAudioStatusSnapshot>> = OnceLock::new();
    LIVE_AUDIO_STATUS.get_or_init(|| Mutex::new(LiveAudioStatusSnapshot::default()))
}

pub fn list_microphone_input_devices() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

pub fn update_live_audio_capture(
    capture_system_audio: bool,
    capture_microphone_audio: bool,
) -> Result<(), String> {
    let mut status = live_audio_status()
        .lock()
        .map_err(|_| "No se pudo actualizar estado de audio".to_string())?;

    status.capture_system_audio = capture_system_audio;
    status.capture_microphone_audio = capture_microphone_audio;
    Ok(())
}

pub fn apply_audio_capture_config(config: &AudioCaptureConfig) {
    if let Ok(mut status) = live_audio_status().lock() {
        status.capture_system_audio = config.capture_system_audio;
        status.capture_microphone_audio = config.capture_microphone_audio;
        status.system_audio_device_name = config.system_audio_device.clone();
        status.microphone_audio_device_name = config.microphone_device.clone();
    }
}

pub fn get_live_audio_status() -> LiveAudioStatusSnapshot {
    live_audio_status()
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default()
}
