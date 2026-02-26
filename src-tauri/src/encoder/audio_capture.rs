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

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use tempfile::tempdir;

    use super::{
        get_live_audio_status, list_microphone_input_devices, update_live_audio_capture,
        AudioCaptureService,
    };
    use crate::encoder::config::{AudioCaptureConfig, OutputFormat};

    #[test]
    fn lista_microfonos_stub_devuelve_vacia() {
        let devices =
            list_microphone_input_devices().expect("listado de microfonos debe responder");
        assert!(devices.is_empty());
    }

    #[test]
    fn update_audio_en_vivo_stub_devuelve_error_controlado() {
        let err = update_live_audio_capture(true, true)
            .expect_err("en no-windows no debe habilitar audio en vivo");
        assert!(err.contains("Windows"));
    }

    #[test]
    fn status_audio_stub_arranca_en_default() {
        let status = get_live_audio_status();
        assert!(!status.capture_system_audio);
        assert!(!status.capture_microphone_audio);
        assert!(status.system_audio_device_name.is_none());
        assert!(status.microphone_audio_device_name.is_none());
    }

    #[test]
    fn servicio_audio_stub_rechaza_audio_habilitado() {
        let temp_dir = tempdir().expect("tempdir");
        let output_path = temp_dir.path().join("video.tmp.mp4");
        let final_path = temp_dir.path().join("video.mp4");
        std::fs::write(&output_path, b"video").expect("escribir archivo temporal");

        let mut service = AudioCaptureService::new(
            AudioCaptureConfig {
                capture_system_audio: true,
                ..AudioCaptureConfig::default()
            },
            OutputFormat::Mp4,
            output_path,
            final_path,
            temp_dir,
        );

        let err = service
            .start()
            .expect_err("en no-windows el audio habilitado debe fallar");
        assert!(err.contains("Windows"));
    }

    #[test]
    fn servicio_audio_stub_permite_sin_audio() {
        let temp_dir = tempdir().expect("tempdir");
        let output_path = temp_dir.path().join("video.tmp.mp4");
        let final_path = temp_dir.path().join("video.mp4");
        std::fs::write(&output_path, b"video").expect("escribir archivo temporal");

        let mut service = AudioCaptureService::new(
            AudioCaptureConfig::default(),
            OutputFormat::Mp4,
            output_path,
            final_path,
            temp_dir,
        );

        assert!(service.start().is_ok());
    }
}
