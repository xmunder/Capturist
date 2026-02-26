use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Instant,
};

use tempfile::TempDir;
use windows::Win32::Media::Audio::{eCapture, eRender, EDataFlow};

use crate::encoder::{
    audio_capture::LiveAudioStatusSnapshot,
    config::{AudioCaptureConfig, OutputFormat},
    output_paths::move_temp_to_final,
    processing_status::ProcessingGuard,
};

use self::{
    device_discovery::{list_microphone_input_devices_impl, resolve_device},
    mux::{audio_file_has_payload, mux_audio_into_video},
    wasapi_capture::{
        normalized_track_delay, spawn_capture_worker, stop_capture_worker, ActiveCapture,
    },
};

mod device_discovery;
mod dsp;
mod mux;
mod wasapi_capture;

#[derive(Clone)]
struct LiveAudioController {
    system_enabled: Option<Arc<AtomicBool>>,
    microphone_enabled: Option<Arc<AtomicBool>>,
    system_device_name: Option<String>,
    microphone_device_name: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioTrackSource {
    System,
    Microphone,
}

pub(super) struct AudioTrackInput {
    pub(super) path: PathBuf,
    pub(super) delay_ms: u64,
    pub(super) source: AudioTrackSource,
}

pub struct AudioCaptureServiceImpl {
    config: AudioCaptureConfig,
    format: OutputFormat,
    output_path: PathBuf,
    final_output_path: PathBuf,
    temp_dir: Option<TempDir>,
    system_capture: Option<ActiveCapture>,
    microphone_capture: Option<ActiveCapture>,
    started: bool,
}

fn live_audio_controller_slot() -> &'static Mutex<Option<LiveAudioController>> {
    static SLOT: OnceLock<Mutex<Option<LiveAudioController>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn set_live_audio_controller(controller: Option<LiveAudioController>) {
    if let Ok(mut guard) = live_audio_controller_slot().lock() {
        *guard = controller;
    }
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
            format,
            output_path,
            final_output_path,
            temp_dir: Some(temp_dir),
            system_capture: None,
            microphone_capture: None,
            started: false,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Ok(());
        }

        if self.temp_dir.is_none() {
            return Err("No se pudo preparar la carpeta temporal de audio.".to_string());
        }
        let recording_started_at = Instant::now();

        let start_result = (|| -> Result<(), String> {
            let temp_base = self
                .temp_dir
                .as_ref()
                .expect("temp_dir inicializado")
                .path()
                .to_path_buf();

            self.system_capture = start_capture_track(
                "audio del sistema",
                eRender,
                self.config.system_audio_device.as_deref(),
                true,
                self.config.capture_system_audio,
                self.config.capture_system_audio,
                temp_base.join("system_audio.wav"),
                recording_started_at,
            )?;

            self.microphone_capture = start_capture_track(
                "audio de micrófono",
                eCapture,
                self.config.microphone_device.as_deref(),
                false,
                self.config.capture_microphone_audio,
                self.config.capture_microphone_audio,
                temp_base.join("microphone_audio.wav"),
                recording_started_at,
            )?;

            self.started = true;
            set_live_audio_controller(Some(LiveAudioController {
                system_enabled: self
                    .system_capture
                    .as_ref()
                    .map(|capture| Arc::clone(&capture.enabled)),
                microphone_enabled: self
                    .microphone_capture
                    .as_ref()
                    .map(|capture| Arc::clone(&capture.enabled)),
                system_device_name: self
                    .system_capture
                    .as_ref()
                    .map(|capture| capture.device_name.clone()),
                microphone_device_name: self
                    .microphone_capture
                    .as_ref()
                    .map(|capture| capture.device_name.clone()),
            }));
            Ok(())
        })();

        if let Err(err) = start_result {
            let mut errors = Vec::new();
            stop_capture_worker(&mut self.system_capture, &mut errors);
            stop_capture_worker(&mut self.microphone_capture, &mut errors);
            self.reset_state();
            return Err(err);
        }

        Ok(())
    }

    pub fn finalize_and_mux(&mut self) -> Result<(), String> {
        if !self.started {
            self.reset_state();
            return Ok(());
        }

        let mut thread_errors = Vec::new();
        stop_capture_worker(&mut self.system_capture, &mut thread_errors);
        stop_capture_worker(&mut self.microphone_capture, &mut thread_errors);

        let mut audio_tracks = Vec::new();
        if let Some(track) = self.system_capture.as_ref() {
            if track.ever_enabled.load(Ordering::SeqCst) && audio_file_has_payload(&track.wav_path)
            {
                audio_tracks.push(AudioTrackInput {
                    path: track.wav_path.clone(),
                    delay_ms: normalized_track_delay(
                        track.first_enabled_at_ms.load(Ordering::SeqCst),
                    ),
                    source: AudioTrackSource::System,
                });
            }
        }
        if let Some(track) = self.microphone_capture.as_ref() {
            if track.ever_enabled.load(Ordering::SeqCst) && audio_file_has_payload(&track.wav_path)
            {
                audio_tracks.push(AudioTrackInput {
                    path: track.wav_path.clone(),
                    delay_ms: normalized_track_delay(
                        track.first_enabled_at_ms.load(Ordering::SeqCst),
                    ),
                    source: AudioTrackSource::Microphone,
                });
            }
        }

        let _processing_guard = ProcessingGuard::start();

        let mux_result = if audio_tracks.is_empty() {
            if self.config.is_enabled() {
                if !thread_errors.is_empty() {
                    for err in &thread_errors {
                        eprintln!("[audio-wasapi] advertencia durante captura: {}", err);
                    }
                }

                let move_err = move_temp_to_final(&self.output_path, &self.final_output_path).err();
                if let Some(err) = move_err {
                    Err(err)
                } else if let Some(err) = thread_errors.into_iter().next() {
                    Err(err)
                } else {
                    Err("No se capturó audio válido durante la grabación.".to_string())
                }
            } else {
                if !thread_errors.is_empty() {
                    for err in &thread_errors {
                        eprintln!("[audio-wasapi] advertencia durante captura: {}", err);
                    }
                }
                move_temp_to_final(&self.output_path, &self.final_output_path)
            }
        } else {
            if !thread_errors.is_empty() {
                for err in &thread_errors {
                    eprintln!("[audio-wasapi] advertencia durante captura: {}", err);
                }
            }
            mux_audio_into_video(
                &self.format,
                &self.output_path,
                &self.final_output_path,
                &audio_tracks,
                self.config.microphone_gain_percent,
            )
        };

        self.reset_state();
        mux_result
    }

    fn reset_state(&mut self) {
        set_live_audio_controller(None);
        self.system_capture = None;
        self.microphone_capture = None;
        self.temp_dir = None;
        self.started = false;
    }
}

pub fn list_microphone_input_devices() -> Result<Vec<String>, String> {
    list_microphone_input_devices_impl()
}

pub fn update_live_audio_capture(
    capture_system_audio: bool,
    capture_microphone_audio: bool,
) -> Result<(), String> {
    let mut guard = live_audio_controller_slot()
        .lock()
        .map_err(|_| "No se pudo sincronizar la actualización de audio en vivo.".to_string())?;

    let controller = guard
        .as_mut()
        .ok_or_else(|| "No hay una grabación activa para actualizar audio".to_string())?;

    if capture_system_audio && controller.system_enabled.is_none() {
        return Err(
            "No hay capturador disponible para audio del sistema en esta sesión.".to_string(),
        );
    }
    if capture_microphone_audio && controller.microphone_enabled.is_none() {
        return Err("No hay capturador disponible para micrófono en esta sesión.".to_string());
    }

    if let Some(flag) = controller.system_enabled.as_ref() {
        flag.store(capture_system_audio, Ordering::SeqCst);
    }
    if let Some(flag) = controller.microphone_enabled.as_ref() {
        flag.store(capture_microphone_audio, Ordering::SeqCst);
    }

    Ok(())
}

pub fn apply_audio_capture_config(_config: &AudioCaptureConfig) {}

pub fn get_live_audio_status() -> LiveAudioStatusSnapshot {
    let guard = live_audio_controller_slot().lock();
    let Ok(guard) = guard else {
        return LiveAudioStatusSnapshot::default();
    };

    let Some(controller) = guard.as_ref() else {
        return LiveAudioStatusSnapshot::default();
    };

    LiveAudioStatusSnapshot {
        capture_system_audio: controller
            .system_enabled
            .as_ref()
            .map(|flag| flag.load(Ordering::SeqCst))
            .unwrap_or(false),
        capture_microphone_audio: controller
            .microphone_enabled
            .as_ref()
            .map(|flag| flag.load(Ordering::SeqCst))
            .unwrap_or(false),
        system_audio_device_name: controller.system_device_name.clone(),
        microphone_audio_device_name: controller.microphone_device_name.clone(),
    }
}

fn start_capture_track(
    kind: &'static str,
    dataflow: EDataFlow,
    preferred_device: Option<&str>,
    loopback: bool,
    required: bool,
    initial_enabled: bool,
    wav_path: PathBuf,
    recording_started_at: Instant,
) -> Result<Option<ActiveCapture>, String> {
    let resolved = resolve_device(dataflow, preferred_device, kind);
    let device = match resolved {
        Ok(device) => device,
        Err(err) if !required => {
            eprintln!(
                "[audio-wasapi] {} opcional no disponible con dispositivo preferido: {}",
                kind, err
            );

            match resolve_device(dataflow, None, kind) {
                Ok(default_device) => default_device,
                Err(default_err) => {
                    eprintln!(
                        "[audio-wasapi] {} tampoco disponible con dispositivo por defecto: {}",
                        kind, default_err
                    );
                    return Ok(None);
                }
            }
        }
        Err(err) => return Err(err),
    };

    spawn_capture_worker(
        kind,
        wav_path,
        device,
        loopback,
        initial_enabled,
        recording_started_at,
    )
    .map(Some)
}
