use std::path::PathBuf;

use tauri::State;

use crate::{
    capture::{
        manager::{CaptureManager, CaptureManagerSnapshot, SessionConfig},
        models::{CaptureResolutionPreset, CaptureState, CaptureTarget, Region},
    },
    encoder::{
        audio_capture::{
            apply_audio_capture_config, get_live_audio_status, list_microphone_input_devices,
            update_live_audio_capture, LiveAudioStatusSnapshot,
        },
        config::{
            AudioCaptureConfig, EncoderConfig, EncoderPreset, OutputFormat, OutputResolution,
            QualityMode, VideoCodec, VideoEncoderPreference,
        },
        processing_status::{is_processing, set_processing},
        video_encoder_status::{
            get_live_video_encoder_label, infer_label_from_preference, set_live_video_encoder_label,
        },
    },
    region,
    shortcuts::ShortcutBindings,
    AppState,
};

const CAPTURE_LOCK_ERR: &str =
    "No se pudo acceder al estado de captura (lock interno en estado inválido)";
const SHORTCUTS_LOCK_ERR: &str =
    "No se pudo acceder al estado de atajos globales (lock interno en estado inválido)";

fn lock_capture<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, CaptureManager>, String> {
    state
        .capture
        .lock()
        .map_err(|_| CAPTURE_LOCK_ERR.to_string())
}

#[tauri::command]
pub fn select_region_native() -> Result<Option<Region>, String> {
    region::select_region()
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionConfig {
    pub target_id: u32,
    pub fps: u32,
    pub crop_region: Option<Region>,
    pub output_path: String,
    pub format: OutputFormat,
    pub codec: Option<VideoCodec>,
    #[serde(default = "default_video_encoder_preference")]
    pub video_encoder_preference: VideoEncoderPreference,
    pub resolution: OutputResolution,
    #[serde(default = "default_crf")]
    pub crf: u32,
    #[serde(default = "default_preset")]
    pub preset: EncoderPreset,
    #[serde(default = "default_quality_mode")]
    pub quality_mode: QualityMode,
    #[serde(default)]
    pub capture_system_audio: bool,
    #[serde(default)]
    pub capture_microphone_audio: bool,
    #[serde(default)]
    pub system_audio_device: Option<String>,
    #[serde(default)]
    pub microphone_device: Option<String>,
    #[serde(default = "default_microphone_gain_percent")]
    pub microphone_gain_percent: u16,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAudioCaptureUpdate {
    pub capture_system_audio: bool,
    pub capture_microphone_audio: bool,
}

fn default_crf() -> u32 {
    23
}

fn default_preset() -> EncoderPreset {
    EncoderPreset::UltraFast
}

fn default_video_encoder_preference() -> VideoEncoderPreference {
    VideoEncoderPreference::Auto
}

fn default_microphone_gain_percent() -> u16 {
    100
}

fn default_quality_mode() -> QualityMode {
    QualityMode::Balanced
}

fn resolve_capture_resolution_preset(
    resolution: &OutputResolution,
    quality_mode: &QualityMode,
) -> Option<CaptureResolutionPreset> {
    if matches!(quality_mode, QualityMode::Quality) {
        return None;
    }

    match resolution {
        OutputResolution::Native => None,
        OutputResolution::FullHd => Some(CaptureResolutionPreset::R1080p),
        OutputResolution::Hd => Some(CaptureResolutionPreset::R720p),
        OutputResolution::Sd => Some(CaptureResolutionPreset::R480p),
        OutputResolution::P1440 => Some(CaptureResolutionPreset::R1440p),
        OutputResolution::P2160 => Some(CaptureResolutionPreset::R2160p),
        OutputResolution::Custom { width, height } => {
            let max_dim = (*width).max(*height);
            if max_dim <= 640 {
                Some(CaptureResolutionPreset::R480p)
            } else if max_dim <= 1280 {
                Some(CaptureResolutionPreset::R720p)
            } else if max_dim <= 1920 {
                Some(CaptureResolutionPreset::R1080p)
            } else if max_dim <= 2560 {
                Some(CaptureResolutionPreset::R1440p)
            } else if max_dim <= 3840 {
                Some(CaptureResolutionPreset::R2160p)
            } else if max_dim <= 7680 {
                Some(CaptureResolutionPreset::R4320p)
            } else {
                None
            }
        }
    }
}

#[tauri::command]
pub fn is_capture_supported(state: State<AppState>) -> bool {
    lock_capture(&state)
        .map(|manager| manager.is_supported())
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_targets(state: State<AppState>) -> Result<Vec<CaptureTarget>, String> {
    lock_capture(&state)?.get_targets()
}

#[tauri::command]
pub fn get_audio_input_devices() -> Result<Vec<String>, String> {
    list_microphone_input_devices()
}

#[tauri::command]
pub fn get_recording_audio_status() -> LiveAudioStatusSnapshot {
    get_live_audio_status()
}

#[tauri::command]
pub fn set_global_shortcuts(
    state: State<AppState>,
    config: ShortcutBindings,
) -> Result<(), String> {
    let guard = state
        .global_shortcuts
        .lock()
        .map_err(|_| SHORTCUTS_LOCK_ERR.to_string())?;

    let manager = guard
        .as_ref()
        .ok_or_else(|| "Gestor de atajos globales no inicializado".to_string())?;

    manager.update(config)
}

#[tauri::command]
pub fn start_recording(
    state: State<AppState>,
    config: RecordingSessionConfig,
) -> Result<(), String> {
    let encoder_config = EncoderConfig {
        output_path: PathBuf::from(&config.output_path),
        format: config.format,
        codec: config.codec,
        video_encoder_preference: config.video_encoder_preference,
        resolution: config.resolution,
        crf: config.crf,
        preset: config.preset,
        quality_mode: config.quality_mode,
        fps: config.fps,
        audio: AudioCaptureConfig {
            capture_system_audio: config.capture_system_audio,
            capture_microphone_audio: config.capture_microphone_audio,
            system_audio_device: config.system_audio_device,
            microphone_device: config.microphone_device,
            microphone_gain_percent: config.microphone_gain_percent,
        },
    };

    encoder_config.validate()?;

    let preferred_encoder_label =
        infer_label_from_preference(&encoder_config.video_encoder_preference);

    apply_audio_capture_config(&encoder_config.audio);
    set_live_video_encoder_label(preferred_encoder_label);
    set_processing(false);

    let session_config = SessionConfig {
        target_id: config.target_id,
        fps: config.fps,
        crop_region: config.crop_region,
        capture_resolution_preset: resolve_capture_resolution_preset(
            &encoder_config.resolution,
            &encoder_config.quality_mode,
        ),
        encoder_config,
    };

    let mut manager = lock_capture(&state)?;
    if let Err(err) = manager.start(session_config) {
        set_live_video_encoder_label(None);
        return Err(err);
    }

    Ok(())
}

#[tauri::command]
pub fn update_recording_audio_capture(
    state: State<AppState>,
    config: RecordingAudioCaptureUpdate,
) -> Result<(), String> {
    let mut manager = lock_capture(&state)?;
    manager.refresh_runtime_state();
    let is_active = manager.is_active();
    if !is_active {
        return Err("No hay una grabación activa para actualizar audio".to_string());
    }

    update_live_audio_capture(config.capture_system_audio, config.capture_microphone_audio)
}

#[tauri::command]
pub fn pause_recording(state: State<AppState>) -> Result<(), String> {
    lock_capture(&state)?.pause()
}

#[tauri::command]
pub fn resume_recording(state: State<AppState>) -> Result<(), String> {
    lock_capture(&state)?.resume()
}

#[tauri::command]
pub fn stop_recording(state: State<AppState>) -> Result<(), String> {
    lock_capture(&state)?.stop()?;
    set_live_video_encoder_label(None);
    set_processing(false);
    Ok(())
}

#[tauri::command]
pub fn cancel_recording(state: State<AppState>) -> Result<(), String> {
    lock_capture(&state)?.cancel()?;
    set_live_video_encoder_label(None);
    set_processing(false);
    Ok(())
}

#[tauri::command]
pub fn get_recording_status(state: State<AppState>) -> CaptureManagerSnapshot {
    match lock_capture(&state) {
        Ok(mut manager) => {
            manager.refresh_runtime_state();
            let mut snapshot = manager.snapshot();
            snapshot.video_encoder_label = get_live_video_encoder_label();
            snapshot.is_processing = is_processing();
            snapshot
        }
        Err(err) => CaptureManagerSnapshot {
            state: CaptureState::Idle,
            elapsed_ms: 0,
            last_error: Some(err),
            video_encoder_label: None,
            is_processing: is_processing(),
        },
    }
}
