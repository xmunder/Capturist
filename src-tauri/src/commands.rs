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
        consumer::detect_video_encoder_capabilities,
        processing_status::{is_processing, set_processing},
        video_encoder_status::{get_live_video_encoder_label, set_live_video_encoder_label},
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
pub fn select_region_native(target: Option<CaptureTarget>) -> Result<Option<Region>, String> {
    let Some(target) = target else {
        return region::select_region();
    };

    let bounds = region::SelectionBounds {
        origin_x: target.origin_x,
        origin_y: target.origin_y,
        width: target.screen_width,
        height: target.screen_height,
    };

    let Some(selected_region) = region::select_region_with_bounds(bounds)? else {
        return Ok(None);
    };

    normalize_native_region_for_target(selected_region, &target).map(Some)
}

fn normalize_native_region_for_target(
    selected_region: Region,
    target: &CaptureTarget,
) -> Result<Region, String> {
    if target.width == 0
        || target.height == 0
        || target.screen_width == 0
        || target.screen_height == 0
    {
        return Err("El target de captura tiene dimensiones invalidas".to_string());
    }

    if selected_region.width == 0 || selected_region.height == 0 {
        return Err("La region seleccionada no tiene un area valida".to_string());
    }

    let source_start_x = selected_region.x.min(target.screen_width.saturating_sub(1));
    let source_start_y = selected_region
        .y
        .min(target.screen_height.saturating_sub(1));
    let source_end_x = selected_region
        .x
        .saturating_add(selected_region.width)
        .clamp(source_start_x.saturating_add(1), target.screen_width);
    let source_end_y = selected_region
        .y
        .saturating_add(selected_region.height)
        .clamp(source_start_y.saturating_add(1), target.screen_height);

    let mapped_start_x =
        scale_coordinate(source_start_x, target.screen_width, target.width).min(target.width - 1);
    let mapped_start_y = scale_coordinate(source_start_y, target.screen_height, target.height)
        .min(target.height - 1);
    let mapped_end_x = scale_coordinate(source_end_x, target.screen_width, target.width)
        .clamp(mapped_start_x.saturating_add(1), target.width);
    let mapped_end_y = scale_coordinate(source_end_y, target.screen_height, target.height)
        .clamp(mapped_start_y.saturating_add(1), target.height);

    Ok(Region {
        x: mapped_start_x,
        y: mapped_start_y,
        width: mapped_end_x.saturating_sub(mapped_start_x),
        height: mapped_end_y.saturating_sub(mapped_start_y),
    })
}

fn scale_coordinate(value: u32, source_extent: u32, target_extent: u32) -> u32 {
    ((value as f64 * target_extent as f64) / source_extent as f64).round() as u32
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEncoderCapabilitiesSnapshot {
    pub nvenc: bool,
    pub amf: bool,
    pub qsv: bool,
    pub software: bool,
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
pub fn get_video_encoder_capabilities() -> VideoEncoderCapabilitiesSnapshot {
    let capabilities = detect_video_encoder_capabilities();
    VideoEncoderCapabilitiesSnapshot {
        nvenc: capabilities.nvenc,
        amf: capabilities.amf,
        qsv: capabilities.qsv,
        software: capabilities.software,
    }
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

#[cfg(test)]
mod tests {
    use super::normalize_native_region_for_target;
    use crate::capture::models::{CaptureTarget, Region, TargetKind};

    fn monitor_target(
        width: u32,
        height: u32,
        screen_width: u32,
        screen_height: u32,
    ) -> CaptureTarget {
        CaptureTarget {
            id: 1,
            name: "Monitor".to_string(),
            width,
            height,
            origin_x: 0,
            origin_y: 0,
            screen_width,
            screen_height,
            is_primary: true,
            kind: TargetKind::Monitor,
        }
    }

    #[test]
    fn normaliza_region_de_monitor_con_escala_dpi() {
        let target = monitor_target(3840, 2160, 1920, 1080);
        let selected_region = Region {
            x: 120,
            y: 45,
            width: 600,
            height: 300,
        };

        let normalized = normalize_native_region_for_target(selected_region, &target)
            .expect("la region debe normalizarse");

        assert_eq!(normalized.x, 240);
        assert_eq!(normalized.y, 90);
        assert_eq!(normalized.width, 1200);
        assert_eq!(normalized.height, 600);
    }

    #[test]
    fn recorta_la_region_al_borde_del_target() {
        let target = monitor_target(1920, 1080, 1920, 1080);
        let selected_region = Region {
            x: 1910,
            y: 1075,
            width: 80,
            height: 40,
        };

        let normalized = normalize_native_region_for_target(selected_region, &target)
            .expect("la region debe ajustarse al borde");

        assert_eq!(normalized.x, 1910);
        assert_eq!(normalized.y, 1075);
        assert_eq!(normalized.width, 10);
        assert_eq!(normalized.height, 5);
    }

    #[test]
    fn rechaza_target_con_dimensiones_invalidas() {
        let target = monitor_target(1920, 1080, 0, 1080);
        let selected_region = Region {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };

        let err = normalize_native_region_for_target(selected_region, &target)
            .expect_err("debe fallar cuando el target es invalido");

        assert!(err.contains("dimensiones invalidas"));
    }
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

    apply_audio_capture_config(&encoder_config.audio);
    // La etiqueta del backend debe reflejar el encoder realmente abierto,
    // no solo la preferencia seleccionada por el usuario.
    set_live_video_encoder_label(None);
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
