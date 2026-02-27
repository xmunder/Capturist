use std::{
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use crate::capture::{
    models::{CaptureResolutionPreset, CaptureState, CaptureTarget, RawFrame, Region},
    provider::{ScreenProvider, WindowsCaptureScreenProvider},
    runtime::{
        self, CaptureRuntimeHandle, FrameArrivedCallback, RuntimeStartConfig,
        SessionFinishedCallback,
    },
};
use crate::encoder::{
    config::{EncoderConfig, VideoCodec, VideoEncoderPreference},
    consumer::FfmpegEncoderConsumer,
};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureManagerSnapshot {
    pub state: CaptureState,
    pub elapsed_ms: u64,
    pub last_error: Option<String>,
    pub video_encoder_label: Option<String>,
    pub is_processing: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    pub target_id: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    pub crop_region: Option<Region>,
    #[serde(default)]
    pub capture_resolution_preset: Option<CaptureResolutionPreset>,
    pub encoder_config: EncoderConfig,
}

fn default_fps() -> u32 {
    30
}

#[derive(Clone)]
pub struct RuntimeFactory {
    builder: std::sync::Arc<RuntimeBuilder>,
}

impl RuntimeFactory {
    pub fn new<F>(builder: F) -> Self
    where
        F: Fn(SessionConfig) -> Result<Box<dyn CaptureRuntimeHandle>, String>
            + Send
            + Sync
            + 'static,
    {
        Self {
            builder: std::sync::Arc::new(builder),
        }
    }

    pub fn build(&self, config: SessionConfig) -> Result<Box<dyn CaptureRuntimeHandle>, String> {
        (self.builder)(config)
    }
}

type RuntimeBuilder =
    dyn Fn(SessionConfig) -> Result<Box<dyn CaptureRuntimeHandle>, String> + Send + Sync;

struct ActiveSession {
    state: CaptureState,
    elapsed_before_pause_ms: u64,
    last_resume_at: Option<Instant>,
    last_error: Option<String>,
    runtime: Option<Box<dyn CaptureRuntimeHandle>>,
}

impl ActiveSession {
    fn new(runtime: Box<dyn CaptureRuntimeHandle>) -> Self {
        Self {
            state: CaptureState::Running,
            elapsed_before_pause_ms: 0,
            last_resume_at: Some(Instant::now()),
            last_error: None,
            runtime: Some(runtime),
        }
    }

    fn accumulate_elapsed(&mut self) {
        if let Some(since) = self.last_resume_at.take() {
            self.elapsed_before_pause_ms += since.elapsed().as_millis() as u64;
        }
    }

    fn elapsed_ms(&self) -> u64 {
        match self.state {
            CaptureState::Running => {
                if let Some(since) = self.last_resume_at {
                    self.elapsed_before_pause_ms + since.elapsed().as_millis() as u64
                } else {
                    self.elapsed_before_pause_ms
                }
            }
            _ => self.elapsed_before_pause_ms,
        }
    }

    fn runtime_finished(&self) -> bool {
        self.runtime
            .as_ref()
            .map(|runtime| runtime.is_finished())
            .unwrap_or(true)
    }
}

pub struct CaptureManager {
    active_session: Option<ActiveSession>,
    provider: Box<dyn ScreenProvider + Send>,
    runtime_factory: RuntimeFactory,
}

impl CaptureManager {
    pub fn new() -> Self {
        Self::with_dependencies(
            Box::new(WindowsCaptureScreenProvider::new()),
            RuntimeFactory::new(|config: SessionConfig| {
                let prefer_gpu_frames =
                    should_prefer_gpu_frames(&config.encoder_config, &config.crop_region);
                let SessionConfig {
                    target_id,
                    fps,
                    crop_region,
                    capture_resolution_preset: _,
                    encoder_config,
                } = config;

                let frame_callbacks = build_runtime_callbacks(encoder_config)?;
                runtime::start_runtime(RuntimeStartConfig {
                    target_id,
                    fps,
                    crop_region,
                    prefer_gpu_frames,
                    should_accept_frame: frame_callbacks.0,
                    on_frame_dropped: frame_callbacks.1,
                    on_frame_arrived: frame_callbacks.2,
                    on_session_finished: frame_callbacks.3,
                })
            }),
        )
    }

    pub fn with_dependencies(
        provider: Box<dyn ScreenProvider + Send>,
        runtime_factory: RuntimeFactory,
    ) -> Self {
        Self {
            active_session: None,
            provider,
            runtime_factory,
        }
    }

    fn cleanup_stopped_session_if_any(&mut self) {
        let should_cleanup = self
            .active_session
            .as_ref()
            .map(|session| session.state == CaptureState::Stopped)
            .unwrap_or(false);

        if should_cleanup {
            self.active_session = None;
        }
    }

    fn finalize_finished_runtime_if_any(&mut self) {
        let should_finalize = self
            .active_session
            .as_ref()
            .map(|session| {
                matches!(session.state, CaptureState::Running | CaptureState::Paused)
                    && session.runtime_finished()
            })
            .unwrap_or(false);

        if !should_finalize {
            return;
        }

        if let Some(session) = self.active_session.as_mut() {
            session.accumulate_elapsed();
            session.state = CaptureState::Stopped;
            session.last_resume_at = None;

            if let Some(runtime) = session.runtime.take() {
                if let Err(err) = runtime.wait() {
                    session.last_error = Some(err);
                }
            }
        }
    }

    pub fn refresh_runtime_state(&mut self) {
        self.finalize_finished_runtime_if_any();
    }

    pub fn get_targets(&self) -> Result<Vec<CaptureTarget>, String> {
        self.provider.get_targets()
    }

    pub fn is_supported(&self) -> bool {
        self.provider.is_supported()
    }

    pub fn start(&mut self, config: SessionConfig) -> Result<(), String> {
        self.finalize_finished_runtime_if_any();
        self.cleanup_stopped_session_if_any();

        if self.active_session.is_some() {
            return Err("Ya existe una grabación en curso".to_string());
        }

        if config.fps == 0 || config.fps > 120 {
            return Err("FPS inválido. Debe estar entre 1 y 120".to_string());
        }

        let target = self
            .get_targets()?
            .into_iter()
            .find(|target| target.id == config.target_id)
            .ok_or_else(|| format!("No se encontró un target con id {}", config.target_id))?;

        if let Some(region) = &config.crop_region {
            region.validate_against_target(&target)?;
        }

        let runtime = self.runtime_factory.build(config)?;
        self.active_session = Some(ActiveSession::new(runtime));
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.finalize_finished_runtime_if_any();

        let session = self
            .active_session
            .as_mut()
            .ok_or_else(|| "No hay una grabación activa".to_string())?;

        if !session.state.can_pause() {
            return Err(format!(
                "Transición inválida: no se puede pausar desde {}",
                session.state
            ));
        }

        if let Some(runtime) = session.runtime.as_ref() {
            runtime.pause();
        }

        session.accumulate_elapsed();
        session.state = CaptureState::Paused;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        self.finalize_finished_runtime_if_any();

        let session = self
            .active_session
            .as_mut()
            .ok_or_else(|| "No hay una grabación activa".to_string())?;

        if !session.state.can_resume() {
            return Err(format!(
                "Transición inválida: no se puede reanudar desde {}",
                session.state
            ));
        }

        if let Some(runtime) = session.runtime.as_ref() {
            runtime.resume();
        }

        session.state = CaptureState::Running;
        session.last_resume_at = Some(Instant::now());
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.finalize_finished_runtime_if_any();

        let mut session = self
            .active_session
            .take()
            .ok_or_else(|| "No hay una grabación activa".to_string())?;

        if session.state.can_stop() {
            session.accumulate_elapsed();
            session.state = CaptureState::Stopped;
        } else if session.state != CaptureState::Stopped {
            self.active_session = Some(session);
            return Err(format!(
                "Transición inválida: no se puede detener desde {}",
                self.active_session
                    .as_ref()
                    .map(|active| active.state.to_string())
                    .unwrap_or_else(|| CaptureState::Idle.to_string())
            ));
        }

        if let Some(runtime) = session.runtime.take() {
            if let Err(err) = runtime.stop() {
                session.last_error = Some(err.clone());
                self.active_session = Some(session);
                return Err(err);
            }
        }

        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        self.stop()
    }

    pub fn snapshot(&self) -> CaptureManagerSnapshot {
        match &self.active_session {
            Some(session) => CaptureManagerSnapshot {
                state: session.state.clone(),
                elapsed_ms: session.elapsed_ms(),
                last_error: session.last_error.clone(),
                video_encoder_label: None,
                is_processing: false,
            },
            None => CaptureManagerSnapshot {
                state: CaptureState::Idle,
                elapsed_ms: 0,
                last_error: None,
                video_encoder_label: None,
                is_processing: false,
            },
        }
    }

    pub fn is_active(&self) -> bool {
        self.active_session
            .as_ref()
            .map(|session| matches!(session.state, CaptureState::Running | CaptureState::Paused))
            .unwrap_or(false)
    }
}

impl Default for CaptureManager {
    fn default() -> Self {
        Self::new()
    }
}

fn should_prefer_gpu_frames(encoder_config: &EncoderConfig, crop_region: &Option<Region>) -> bool {
    should_prefer_gpu_frames_with_flag(
        encoder_config,
        crop_region,
        is_experimental_d3d11_input_enabled(),
    )
}

fn should_prefer_gpu_frames_with_flag(
    encoder_config: &EncoderConfig,
    crop_region: &Option<Region>,
    d3d11_input_enabled: bool,
) -> bool {
    // Ruta experimental: sin AVHWFramesContext completo algunos drivers/encoders
    // rechazan AV_PIX_FMT_D3D11 con "Invalid argument".
    if !d3d11_input_enabled {
        return false;
    }

    if crop_region.is_some() {
        return false;
    }

    let codec = encoder_config.effective_codec();
    if matches!(codec, VideoCodec::Vp9) {
        return false;
    }

    matches!(
        encoder_config.video_encoder_preference,
        VideoEncoderPreference::Nvenc | VideoEncoderPreference::Amf | VideoEncoderPreference::Qsv
    )
}

fn is_experimental_d3d11_input_enabled() -> bool {
    match std::env::var("CAPTURIST_EXPERIMENTAL_D3D11_INPUT") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes"
        }
        Err(_) => false,
    }
}

const VIDEO_PIPELINE_QUEUE_CAPACITY: usize = 6;

enum VideoWorkerMessage {
    Frame(RawFrame),
    Stop,
}

struct AsyncVideoPipeline {
    sender: SyncSender<VideoWorkerMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
    worker_error: Arc<Mutex<Option<String>>>,
    queued_frames: Arc<AtomicUsize>,
    dropped_frames: AtomicU64,
}

fn build_runtime_callbacks(
    encoder_config: EncoderConfig,
) -> Result<
    (
        runtime::ShouldAcceptFrameCallback,
        runtime::FrameDroppedCallback,
        FrameArrivedCallback,
        SessionFinishedCallback,
    ),
    String,
> {
    let (sender, receiver) =
        mpsc::sync_channel::<VideoWorkerMessage>(VIDEO_PIPELINE_QUEUE_CAPACITY);
    let worker_error = Arc::new(Mutex::new(None::<String>));
    let worker_error_for_thread = Arc::clone(&worker_error);
    let queued_frames = Arc::new(AtomicUsize::new(0));
    let queued_frames_for_thread = Arc::clone(&queued_frames);

    let worker = thread::Builder::new()
        .name("video-encoder-worker".to_string())
        .spawn(move || {
            configure_video_worker_thread();

            let mut consumer = match FfmpegEncoderConsumer::new(encoder_config) {
                Ok(consumer) => consumer,
                Err(err) => {
                    set_worker_error(&worker_error_for_thread, err);
                    return;
                }
            };

            while let Ok(message) = receiver.recv() {
                match message {
                    VideoWorkerMessage::Frame(raw_frame) => {
                        decrement_queued_frames(&queued_frames_for_thread);
                        if let Err(err) = consumer.on_frame(raw_frame) {
                            set_worker_error(
                                &worker_error_for_thread,
                                format!("Error codificando frame de video: {err}"),
                            );
                            break;
                        }
                    }
                    VideoWorkerMessage::Stop => break,
                }
            }

            if let Err(err) = consumer.on_stop() {
                set_worker_error(
                    &worker_error_for_thread,
                    format!("Error cerrando encoder de video: {err}"),
                );
            }
        })
        .map_err(|err| format!("No se pudo crear worker de codificación de video: {err}"))?;

    let pipeline = Arc::new(AsyncVideoPipeline {
        sender,
        worker: Mutex::new(Some(worker)),
        worker_error,
        queued_frames,
        dropped_frames: AtomicU64::new(0),
    });

    let should_accept_frame: runtime::ShouldAcceptFrameCallback = {
        let pipeline = Arc::clone(&pipeline);
        Arc::new(move || {
            if let Some(err) = read_worker_error(&pipeline.worker_error)? {
                return Err(err);
            }

            let queued = pipeline.queued_frames.load(Ordering::Acquire);
            Ok(queued < VIDEO_PIPELINE_QUEUE_CAPACITY)
        })
    };

    let on_frame_dropped: runtime::FrameDroppedCallback = {
        let pipeline = Arc::clone(&pipeline);
        Arc::new(move || {
            pipeline.dropped_frames.fetch_add(1, Ordering::Relaxed);
        })
    };

    let frame_callback: FrameArrivedCallback = {
        let pipeline = Arc::clone(&pipeline);
        Arc::new(move |raw_frame| {
            if let Some(err) = read_worker_error(&pipeline.worker_error)? {
                return Err(err);
            }

            pipeline.queued_frames.fetch_add(1, Ordering::AcqRel);
            match pipeline
                .sender
                .try_send(VideoWorkerMessage::Frame(raw_frame))
            {
                Ok(()) => Ok(()),
                Err(TrySendError::Full(_)) => {
                    decrement_queued_frames(&pipeline.queued_frames);
                    // Mantiene la captura fluida cuando el encoder va atrasado.
                    pipeline.dropped_frames.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
                Err(TrySendError::Disconnected(_)) => {
                    decrement_queued_frames(&pipeline.queued_frames);
                    if let Some(err) = read_worker_error(&pipeline.worker_error)? {
                        return Err(err);
                    }
                    Err("El worker de codificación de video se desconectó".to_string())
                }
            }
        })
    };

    let session_finished_callback: SessionFinishedCallback = {
        let pipeline = Arc::clone(&pipeline);
        Arc::new(move || {
            let _ = pipeline.sender.send(VideoWorkerMessage::Stop);

            let worker = pipeline
                .worker
                .lock()
                .map_err(|_| {
                    "No se pudo adquirir lock para esperar worker de codificación".to_string()
                })?
                .take();

            if let Some(worker) = worker {
                if worker.join().is_err() {
                    set_worker_error(
                        &pipeline.worker_error,
                        "El worker de codificación de video finalizó con panic".to_string(),
                    );
                }
            }

            let dropped = pipeline.dropped_frames.load(Ordering::Relaxed);
            if dropped > 0 {
                eprintln!(
                    "[capture] Se descartaron {dropped} frames por backpressure del encoder."
                );
            }

            if let Some(err) = take_worker_error(&pipeline.worker_error)? {
                return Err(err);
            }

            Ok(())
        })
    };

    Ok((
        should_accept_frame,
        on_frame_dropped,
        frame_callback,
        session_finished_callback,
    ))
}

#[cfg(target_os = "windows")]
fn configure_video_worker_thread() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };

    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(not(target_os = "windows"))]
fn configure_video_worker_thread() {}

fn decrement_queued_frames(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        Some(value.saturating_sub(1))
    });
}

fn read_worker_error(error_slot: &Arc<Mutex<Option<String>>>) -> Result<Option<String>, String> {
    error_slot
        .lock()
        .map_err(|_| "No se pudo adquirir lock del estado de error del encoder".to_string())
        .map(|guard| guard.clone())
}

fn take_worker_error(error_slot: &Arc<Mutex<Option<String>>>) -> Result<Option<String>, String> {
    error_slot
        .lock()
        .map_err(|_| "No se pudo adquirir lock del estado de error del encoder".to_string())
        .map(|mut guard| guard.take())
}

fn set_worker_error(error_slot: &Arc<Mutex<Option<String>>>, message: String) {
    if let Ok(mut guard) = error_slot.lock() {
        match guard.as_mut() {
            Some(existing) => {
                existing.push_str(" | ");
                existing.push_str(&message);
            }
            None => {
                *guard = Some(message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use super::*;
    use crate::capture::models::TargetKind;
    use crate::encoder::config::{VideoCodec, VideoEncoderPreference};

    struct MockScreenProvider {
        supported: bool,
        targets: Vec<CaptureTarget>,
    }

    impl MockScreenProvider {
        fn with_single_monitor() -> Self {
            Self {
                supported: true,
                targets: vec![CaptureTarget {
                    id: 1,
                    name: "Monitor de prueba".to_string(),
                    width: 1920,
                    height: 1080,
                    origin_x: 0,
                    origin_y: 0,
                    screen_width: 1920,
                    screen_height: 1080,
                    is_primary: true,
                    kind: TargetKind::Monitor,
                }],
            }
        }
    }

    impl ScreenProvider for MockScreenProvider {
        fn get_targets(&self) -> Result<Vec<CaptureTarget>, String> {
            Ok(self.targets.clone())
        }

        fn is_supported(&self) -> bool {
            self.supported
        }
    }

    struct MockRuntimeHandle {
        paused: Arc<AtomicBool>,
        finished: Arc<AtomicBool>,
    }

    impl MockRuntimeHandle {
        fn new() -> Self {
            Self {
                paused: Arc::new(AtomicBool::new(false)),
                finished: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl CaptureRuntimeHandle for MockRuntimeHandle {
        fn pause(&self) {
            self.paused.store(true, Ordering::Relaxed);
        }

        fn resume(&self) {
            self.paused.store(false, Ordering::Relaxed);
        }

        fn is_finished(&self) -> bool {
            self.finished.load(Ordering::Relaxed)
        }

        fn stop(self: Box<Self>) -> Result<u64, String> {
            self.finished.store(true, Ordering::Relaxed);
            Ok(0)
        }

        fn wait(self: Box<Self>) -> Result<u64, String> {
            self.finished.store(true, Ordering::Relaxed);
            Ok(0)
        }
    }

    fn make_mock_manager() -> CaptureManager {
        CaptureManager::with_dependencies(
            Box::new(MockScreenProvider::with_single_monitor()),
            RuntimeFactory::new(|_config| Ok(Box::new(MockRuntimeHandle::new()))),
        )
    }

    fn make_session_config(target_id: u32) -> SessionConfig {
        SessionConfig {
            target_id,
            fps: 30,
            crop_region: None,
            capture_resolution_preset: None,
            encoder_config: EncoderConfig::default(),
        }
    }

    #[test]
    fn manager_nuevo_esta_en_idle() {
        let manager = make_mock_manager();
        let snapshot = manager.snapshot();

        assert_eq!(snapshot.state, CaptureState::Idle);
        assert_eq!(snapshot.elapsed_ms, 0);
        assert!(snapshot.last_error.is_none());
    }

    #[test]
    fn refleja_si_el_backend_esta_soportado() {
        let manager = make_mock_manager();
        assert!(manager.is_supported());
    }

    #[test]
    fn start_pause_resume_stop_actualiza_estado() {
        let mut manager = make_mock_manager();

        manager.start(make_session_config(1)).unwrap();
        assert_eq!(manager.snapshot().state, CaptureState::Running);

        manager.pause().unwrap();
        assert_eq!(manager.snapshot().state, CaptureState::Paused);

        manager.resume().unwrap();
        assert_eq!(manager.snapshot().state, CaptureState::Running);

        manager.stop().unwrap();
        assert_eq!(manager.snapshot().state, CaptureState::Idle);
    }

    #[test]
    fn no_puede_iniciar_dos_veces() {
        let mut manager = make_mock_manager();

        manager.start(make_session_config(1)).unwrap();
        let err = manager.start(make_session_config(1)).unwrap_err();

        assert!(err.contains("grabación en curso"));
    }

    #[test]
    fn start_con_target_inexistente_falla() {
        let mut manager = make_mock_manager();

        let err = manager.start(make_session_config(999)).unwrap_err();

        assert!(err.contains("No se encontró un target"));
    }

    #[test]
    fn prefiere_frames_gpu_solo_en_hw_explicito_y_sin_crop() {
        let config = EncoderConfig {
            video_encoder_preference: VideoEncoderPreference::Nvenc,
            ..EncoderConfig::default()
        };
        assert!(should_prefer_gpu_frames_with_flag(&config, &None, true));
    }

    #[test]
    fn no_prefiere_frames_gpu_en_auto_para_preservar_fallback_cpu() {
        let config = EncoderConfig {
            video_encoder_preference: VideoEncoderPreference::Auto,
            ..EncoderConfig::default()
        };
        assert!(!should_prefer_gpu_frames_with_flag(&config, &None, true));
    }

    #[test]
    fn no_prefiere_frames_gpu_con_crop_ni_vp9() {
        let config = EncoderConfig {
            video_encoder_preference: VideoEncoderPreference::Nvenc,
            codec: Some(VideoCodec::Vp9),
            ..EncoderConfig::default()
        };
        assert!(!should_prefer_gpu_frames_with_flag(&config, &None, true));
        assert!(!should_prefer_gpu_frames_with_flag(
            &EncoderConfig {
                video_encoder_preference: VideoEncoderPreference::Nvenc,
                ..EncoderConfig::default()
            },
            &Some(Region {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            }),
            true,
        ));
    }

    #[test]
    fn no_prefiere_frames_gpu_si_feature_experimental_esta_deshabilitada() {
        let config = EncoderConfig {
            video_encoder_preference: VideoEncoderPreference::Nvenc,
            ..EncoderConfig::default()
        };
        assert!(!should_prefer_gpu_frames_with_flag(&config, &None, false));
    }
}
