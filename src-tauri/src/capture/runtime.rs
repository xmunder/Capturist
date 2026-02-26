use std::sync::Arc;

use crate::capture::models::{RawFrame, Region};

pub type FrameArrivedCallback = Arc<dyn Fn(RawFrame) -> Result<(), String> + Send + Sync>;
pub type SessionFinishedCallback = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct RuntimeStartConfig {
    pub target_id: u32,
    pub fps: u32,
    pub crop_region: Option<Region>,
    pub on_frame_arrived: FrameArrivedCallback,
    pub on_session_finished: SessionFinishedCallback,
}

pub trait CaptureRuntimeHandle: Send {
    fn pause(&self);
    fn resume(&self);
    fn is_finished(&self) -> bool;
    fn stop(self: Box<Self>) -> Result<u64, String>;
    fn wait(self: Box<Self>) -> Result<u64, String>;
}

pub fn start_runtime(config: RuntimeStartConfig) -> Result<Box<dyn CaptureRuntimeHandle>, String> {
    platform::start_runtime(config)
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc,
        },
        time::Duration,
    };

    use windows_capture::{
        capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
        frame::Frame,
        graphics_capture_api::InternalCaptureControl,
        monitor::Monitor,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
        window::Window,
    };

    use crate::capture::{
        models::{RawFrame, Region},
        runtime::{
            CaptureRuntimeHandle, FrameArrivedCallback, RuntimeStartConfig, SessionFinishedCallback,
        },
    };

    const MONITOR_SALT: u64 = 0x045D_9F3B;
    const WINDOW_SALT: u64 = 0x27D4_EB2D;

    pub fn start_runtime(
        config: RuntimeStartConfig,
    ) -> Result<Box<dyn CaptureRuntimeHandle>, String> {
        let paused = Arc::new(AtomicBool::new(false));
        let frame_counter = Arc::new(AtomicU64::new(0));

        let flags = HandlerFlags {
            paused: paused.clone(),
            frame_counter: frame_counter.clone(),
            crop_region: config.crop_region,
            on_frame_arrived: config.on_frame_arrived,
        };

        let min_update_interval_ms = ((1000_u64) / (config.fps.max(1) as u64)).max(1);
        let min_update_interval =
            MinimumUpdateIntervalSettings::Custom(Duration::from_millis(min_update_interval_ms));

        let control = match resolve_capture_item(config.target_id)? {
            CaptureItem::Monitor(monitor) => {
                let settings = Settings::new(
                    monitor,
                    CursorCaptureSettings::WithCursor,
                    DrawBorderSettings::Default,
                    SecondaryWindowSettings::Default,
                    min_update_interval,
                    DirtyRegionSettings::Default,
                    ColorFormat::Bgra8,
                    flags,
                );

                LiveCaptureHandler::start_free_threaded(settings)
                    .map_err(|err| format!("No se pudo iniciar captura en monitor: {err}"))?
            }
            CaptureItem::Window(window) => {
                let settings = Settings::new(
                    window,
                    CursorCaptureSettings::WithCursor,
                    DrawBorderSettings::Default,
                    SecondaryWindowSettings::Default,
                    min_update_interval,
                    DirtyRegionSettings::Default,
                    ColorFormat::Bgra8,
                    flags,
                );

                LiveCaptureHandler::start_free_threaded(settings)
                    .map_err(|err| format!("No se pudo iniciar captura en ventana: {err}"))?
            }
        };

        Ok(Box::new(WindowsCaptureRuntime {
            control: Some(control),
            paused,
            frame_counter,
            on_session_finished: Some(config.on_session_finished),
        }))
    }

    enum CaptureItem {
        Monitor(Monitor),
        Window(Window),
    }

    fn resolve_capture_item(target_id: u32) -> Result<CaptureItem, String> {
        let monitors = Monitor::enumerate()
            .map_err(|err| format!("No se pudieron enumerar monitores: {err}"))?;
        for monitor in monitors {
            let stable_id =
                stable_target_id(monitor.as_raw_hmonitor() as usize as u64, MONITOR_SALT);
            if stable_id == target_id {
                return Ok(CaptureItem::Monitor(monitor));
            }
        }

        let windows = Window::enumerate()
            .map_err(|err| format!("No se pudieron enumerar ventanas: {err}"))?;
        for window in windows {
            let title = match window.title() {
                Ok(value) => value.trim().to_string(),
                Err(_) => continue,
            };

            if title.is_empty() {
                continue;
            }

            let stable_id = stable_target_id(window.as_raw_hwnd() as usize as u64, WINDOW_SALT);
            if stable_id == target_id {
                return Ok(CaptureItem::Window(window));
            }
        }

        Err(format!(
            "No se encontró un target activo con id {} para iniciar captura",
            target_id
        ))
    }

    fn stable_target_id(base: u64, salt: u64) -> u32 {
        let mut value = base ^ salt;
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        value ^= value >> 33;

        (value as u32).max(1)
    }

    struct HandlerFlags {
        paused: Arc<AtomicBool>,
        frame_counter: Arc<AtomicU64>,
        crop_region: Option<Region>,
        on_frame_arrived: FrameArrivedCallback,
    }

    struct LiveCaptureHandler {
        flags: HandlerFlags,
    }

    impl GraphicsCaptureApiHandler for LiveCaptureHandler {
        type Flags = HandlerFlags;
        type Error = String;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self { flags: ctx.flags })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if self.flags.paused.load(Ordering::Relaxed) {
                return Ok(());
            }

            let frame_width = frame.width();
            let frame_height = frame.height();
            let timestamp_ms = frame_timestamp_ms(frame);

            let mut frame_buffer = if let Some(region) = &self.flags.crop_region {
                let (start_x, start_y, end_x, end_y) =
                    clamp_crop_region(region, frame_width, frame_height)?;
                frame
                    .buffer_crop(start_x, start_y, end_x, end_y)
                    .map_err(|err| format!("Error extrayendo frame recortado: {err}"))?
            } else {
                frame
                    .buffer()
                    .map_err(|err| format!("Error extrayendo frame de captura: {err}"))?
            };

            let width = frame_buffer.width();
            let height = frame_buffer.height();

            let bytes = frame_buffer
                .as_nopadding_buffer()
                .map_err(|err| format!("Error normalizando buffer del frame: {err}"))?;

            if bytes.is_empty() {
                return Err("Se recibió un frame vacío desde windows-capture".to_string());
            }

            let raw_frame = RawFrame::new(bytes.to_vec(), width, height, timestamp_ms);
            (self.flags.on_frame_arrived)(raw_frame)
                .map_err(|err| format!("Error procesando frame en encoder: {err}"))?;

            self.flags.frame_counter.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn frame_timestamp_ms(frame: &Frame) -> u64 {
        let raw_duration_100ns = frame.timestamp().Duration;
        if raw_duration_100ns <= 0 {
            return 0;
        }

        (raw_duration_100ns as u64) / 10_000
    }

    fn clamp_crop_region(
        region: &Region,
        frame_width: u32,
        frame_height: u32,
    ) -> Result<(u32, u32, u32, u32), String> {
        if frame_width == 0 || frame_height == 0 {
            return Err("Frame inválido: dimensiones 0x0".to_string());
        }

        let start_x = region.x.min(frame_width - 1);
        let start_y = region.y.min(frame_height - 1);

        let end_x = region.x.saturating_add(region.width).min(frame_width);
        let end_y = region.y.saturating_add(region.height).min(frame_height);

        if end_x <= start_x || end_y <= start_y {
            return Err(
                "La región de recorte no intersecta con el frame capturado en tiempo real"
                    .to_string(),
            );
        }

        Ok((start_x, start_y, end_x, end_y))
    }

    struct WindowsCaptureRuntime {
        control: Option<CaptureControl<LiveCaptureHandler, String>>,
        paused: Arc<AtomicBool>,
        frame_counter: Arc<AtomicU64>,
        on_session_finished: Option<SessionFinishedCallback>,
    }

    impl WindowsCaptureRuntime {
        fn finalize_encoder(&mut self) -> Result<(), String> {
            if let Some(callback) = self.on_session_finished.take() {
                callback()?;
            }
            Ok(())
        }
    }

    impl CaptureRuntimeHandle for WindowsCaptureRuntime {
        fn pause(&self) {
            self.paused.store(true, Ordering::Relaxed);
        }

        fn resume(&self) {
            self.paused.store(false, Ordering::Relaxed);
        }

        fn is_finished(&self) -> bool {
            self.control
                .as_ref()
                .map(CaptureControl::is_finished)
                .unwrap_or(true)
        }

        fn stop(mut self: Box<Self>) -> Result<u64, String> {
            let stop_result = match self.control.take() {
                Some(control) => control
                    .stop()
                    .map_err(|err| format!("Error deteniendo sesión de windows-capture: {err}")),
                None => Err("Control de captura no disponible para detener sesión".to_string()),
            };

            let finalize_result = self.finalize_encoder();

            match (stop_result, finalize_result) {
                (Ok(()), Ok(())) => Ok(self.frame_counter.load(Ordering::Relaxed)),
                (Err(stop_err), Ok(())) => Err(stop_err),
                (Ok(()), Err(finalize_err)) => Err(finalize_err),
                (Err(stop_err), Err(finalize_err)) => Err(format!(
                    "{stop_err}. Además falló la finalización del encoder: {finalize_err}"
                )),
            }
        }

        fn wait(mut self: Box<Self>) -> Result<u64, String> {
            let wait_result = match self.control.take() {
                Some(control) => control.wait().map_err(|err| {
                    format!("Error esperando finalización de windows-capture: {err}")
                }),
                None => Err("Control de captura no disponible para esperar sesión".to_string()),
            };

            let finalize_result = self.finalize_encoder();

            match (wait_result, finalize_result) {
                (Ok(()), Ok(())) => Ok(self.frame_counter.load(Ordering::Relaxed)),
                (Err(wait_err), Ok(())) => Err(wait_err),
                (Ok(()), Err(finalize_err)) => Err(finalize_err),
                (Err(wait_err), Err(finalize_err)) => Err(format!(
                    "{wait_err}. Además falló la finalización del encoder: {finalize_err}"
                )),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::capture::runtime::{CaptureRuntimeHandle, RuntimeStartConfig};

    pub fn start_runtime(
        _config: RuntimeStartConfig,
    ) -> Result<Box<dyn CaptureRuntimeHandle>, String> {
        Err("La captura de pantalla real solo está disponible en Windows".to_string())
    }
}
