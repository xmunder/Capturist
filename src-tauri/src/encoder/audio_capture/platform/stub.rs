#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::PathBuf;

use tempfile::TempDir;

use crate::{
    encoder::audio_capture::LiveAudioStatusSnapshot,
    encoder::{
        config::{AudioCaptureConfig, OutputFormat},
        output_paths::move_temp_to_final,
        processing_status::ProcessingGuard,
    },
};

pub struct AudioCaptureServiceImpl {
    config: AudioCaptureConfig,
    _format: OutputFormat,
    output_path: PathBuf,
    final_output_path: PathBuf,
    _temp_dir: TempDir,
}

impl AudioCaptureServiceImpl {
    pub fn new(
        config: AudioCaptureConfig,
        format: OutputFormat,
        output_path: PathBuf,
        final_output_path: PathBuf,
        temp_dir: TempDir,
    ) -> Self {
        Self {
            config,
            _format: format,
            output_path,
            final_output_path,
            _temp_dir: temp_dir,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.config.is_enabled() {
            return Err("La captura de audio WASAPI solo está disponible en Windows.".to_string());
        }
        Ok(())
    }

    pub fn finalize_and_mux(&mut self) -> Result<(), String> {
        let _processing_guard = ProcessingGuard::start();
        move_temp_to_final(&self.output_path, &self.final_output_path)
    }
}

pub fn list_microphone_input_devices() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

pub fn update_live_audio_capture(
    _capture_system_audio: bool,
    _capture_microphone_audio: bool,
) -> Result<(), String> {
    Err("La actualización de audio en vivo solo está disponible en Windows.".to_string())
}

pub fn apply_audio_capture_config(_config: &AudioCaptureConfig) {}

pub fn get_live_audio_status() -> LiveAudioStatusSnapshot {
    LiveAudioStatusSnapshot::default()
}
