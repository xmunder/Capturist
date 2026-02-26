use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::capture::{
    models::{CaptureResolutionPreset, CaptureState, CaptureTarget, Region},
    provider::{ScreenProvider, WindowsCaptureScreenProvider},
    runtime::{
        self, CaptureRuntimeHandle, FrameArrivedCallback, RuntimeStartConfig,
        SessionFinishedCallback,
    },
};
use crate::encoder::{config::EncoderConfig, consumer::FfmpegEncoderConsumer};

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
                let frame_callbacks = build_runtime_callbacks(config.encoder_config)?;
                runtime::start_runtime(RuntimeStartConfig {
                    target_id: config.target_id,
                    fps: config.fps,
                    crop_region: config.crop_region,
                    on_frame_arrived: frame_callbacks.0,
                    on_session_finished: frame_callbacks.1,
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

fn build_runtime_callbacks(
    encoder_config: EncoderConfig,
) -> Result<(FrameArrivedCallback, SessionFinishedCallback), String> {
    let consumer = Arc::new(Mutex::new(FfmpegEncoderConsumer::new(encoder_config)?));

    let frame_callback: FrameArrivedCallback = {
        let consumer = Arc::clone(&consumer);
        Arc::new(move |raw_frame| {
            let mut guard = consumer
                .lock()
                .map_err(|_| "No se pudo adquirir lock del encoder de video".to_string())?;
            guard.on_frame(raw_frame)
        })
    };

    let session_finished_callback: SessionFinishedCallback = {
        let consumer = Arc::clone(&consumer);
        Arc::new(move || {
            let mut guard = consumer
                .lock()
                .map_err(|_| "No se pudo adquirir lock para cerrar encoder de video".to_string())?;
            guard.on_stop()
        })
    };

    Ok((frame_callback, session_finished_callback))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    use super::*;
    use crate::capture::models::TargetKind;

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
}
