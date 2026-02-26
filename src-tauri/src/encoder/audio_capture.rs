#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::PathBuf;

use tempfile::TempDir;

use crate::encoder::config::{AudioCaptureConfig, OutputFormat};

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAudioStatusSnapshot {
    pub capture_system_audio: bool,
    pub capture_microphone_audio: bool,
    pub system_audio_device_name: Option<String>,
    pub microphone_audio_device_name: Option<String>,
}

pub struct AudioCaptureService {
    inner: platform::AudioCaptureServiceImpl,
}

impl AudioCaptureService {
    pub fn new(
        config: AudioCaptureConfig,
        format: OutputFormat,
        output_path: PathBuf,
        final_output_path: PathBuf,
        temp_dir: TempDir,
    ) -> Self {
        Self {
            inner: platform::AudioCaptureServiceImpl::new(
                config,
                format,
                output_path,
                final_output_path,
                temp_dir,
            ),
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        self.inner.start()
    }

    pub fn finalize_and_mux_detached(mut self) {
        std::thread::spawn(move || {
            if let Err(err) = self.inner.finalize_and_mux() {
                eprintln!("[audio] Error en mux de audio: {err}");
            }
        });
    }
}

pub fn list_microphone_input_devices() -> Result<Vec<String>, String> {
    platform::list_microphone_input_devices()
}

pub fn update_live_audio_capture(
    capture_system_audio: bool,
    capture_microphone_audio: bool,
) -> Result<(), String> {
    platform::update_live_audio_capture(capture_system_audio, capture_microphone_audio)
}

pub fn apply_audio_capture_config(config: &AudioCaptureConfig) {
    platform::apply_audio_capture_config(config);
}

pub fn get_live_audio_status() -> LiveAudioStatusSnapshot {
    platform::get_live_audio_status()
}

#[cfg(windows)]
#[path = "audio_capture/platform/windows.rs"]
mod platform;

#[cfg(not(windows))]
#[path = "audio_capture/platform/stub.rs"]
mod platform;
