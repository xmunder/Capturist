use crate::capture::models::CaptureTarget;
#[cfg(any(target_os = "windows", test))]
use crate::capture::models::TargetKind;

pub trait ScreenProvider {
    fn get_targets(&self) -> Result<Vec<CaptureTarget>, String>;
    fn is_supported(&self) -> bool;
}

pub struct WindowsCaptureScreenProvider;

impl WindowsCaptureScreenProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsCaptureScreenProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenProvider for WindowsCaptureScreenProvider {
    fn get_targets(&self) -> Result<Vec<CaptureTarget>, String> {
        platform::get_targets()
    }

    fn is_supported(&self) -> bool {
        platform::is_supported()
    }
}

#[cfg(any(target_os = "windows", test))]
fn kind_rank(kind: &TargetKind) -> u8 {
    match kind {
        TargetKind::Monitor => 0,
        TargetKind::Window => 1,
    }
}

#[cfg(any(target_os = "windows", test))]
fn sort_targets(mut targets: Vec<CaptureTarget>) -> Vec<CaptureTarget> {
    targets.sort_by(|left, right| {
        kind_rank(&left.kind)
            .cmp(&kind_rank(&right.kind))
            .then_with(|| right.is_primary.cmp(&left.is_primary))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });

    targets
}

#[cfg(target_os = "windows")]
mod platform {
    use std::ffi::c_void;

    use windows_capture::{monitor::Monitor, window::Window};
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO},
    };

    use crate::capture::{
        models::{CaptureTarget, TargetKind},
        provider::sort_targets,
    };

    const MONITOR_SALT: u64 = 0x045D_9F3B;
    const WINDOW_SALT: u64 = 0x27D4_EB2D;

    pub fn is_supported() -> bool {
        Monitor::enumerate()
            .map(|monitors| !monitors.is_empty())
            .unwrap_or(false)
    }

    pub fn get_targets() -> Result<Vec<CaptureTarget>, String> {
        let mut targets = Vec::<CaptureTarget>::new();

        let primary_monitor = Monitor::primary()
            .ok()
            .map(|monitor| monitor.as_raw_hmonitor() as usize);

        let monitors = Monitor::enumerate()
            .map_err(|err| format!("No se pudieron enumerar monitores: {err}"))?;

        for monitor in monitors {
            let raw_handle = monitor.as_raw_hmonitor();

            let (origin_x, origin_y, screen_width, screen_height) =
                monitor_bounds(raw_handle).unwrap_or((0, 0, 1920, 1080));

            let width = monitor.width().unwrap_or(screen_width).max(1);
            let height = monitor.height().unwrap_or(screen_height).max(1);

            let name = monitor
                .name()
                .or_else(|_| monitor.device_name())
                .unwrap_or_else(|_| "Monitor".to_string());

            targets.push(CaptureTarget {
                id: stable_target_id(raw_handle as usize as u64, MONITOR_SALT),
                name,
                width,
                height,
                origin_x,
                origin_y,
                screen_width,
                screen_height,
                is_primary: primary_monitor == Some(raw_handle as usize),
                kind: TargetKind::Monitor,
            });
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

            let rect = match window.rect() {
                Ok(value) => value,
                Err(_) => continue,
            };

            let width = (rect.right - rect.left).max(1) as u32;
            let height = (rect.bottom - rect.top).max(1) as u32;

            targets.push(CaptureTarget {
                id: stable_target_id(window.as_raw_hwnd() as usize as u64, WINDOW_SALT),
                name: title,
                width,
                height,
                origin_x: rect.left,
                origin_y: rect.top,
                screen_width: width,
                screen_height: height,
                is_primary: false,
                kind: TargetKind::Window,
            });
        }

        if targets.is_empty() {
            return Err("No se encontraron fuentes de captura disponibles".to_string());
        }

        Ok(sort_targets(targets))
    }

    fn stable_target_id(base: u64, salt: u64) -> u32 {
        // Mezcla estable sin depender del hasher del proceso.
        let mut value = base ^ salt;
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51_afd7_ed55_8ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        value ^= value >> 33;

        (value as u32).max(1)
    }

    fn monitor_bounds(raw_monitor: *mut c_void) -> Result<(i32, i32, u32, u32), String> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            dwFlags: 0,
        };

        // SAFETY: llamada Win32 de solo lectura sobre un HMONITOR válido entregado por Windows.
        let ok = unsafe { GetMonitorInfoW(raw_monitor as HMONITOR, &mut info as *mut MONITORINFO) };
        if ok == 0 {
            return Err("No se pudo obtener geometría del monitor".to_string());
        }

        let width = (info.rcMonitor.right - info.rcMonitor.left).max(1) as u32;
        let height = (info.rcMonitor.bottom - info.rcMonitor.top).max(1) as u32;

        Ok((info.rcMonitor.left, info.rcMonitor.top, width, height))
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use crate::capture::models::CaptureTarget;

    pub fn is_supported() -> bool {
        false
    }

    pub fn get_targets() -> Result<Vec<CaptureTarget>, String> {
        Err("El backend windows-capture solo está disponible en Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::sort_targets;
    use crate::capture::models::{CaptureTarget, TargetKind};

    #[test]
    fn ordena_monitores_antes_que_ventanas_y_prioriza_monitor_principal() {
        let targets = vec![
            CaptureTarget {
                id: 4,
                name: "Ventana Z".to_string(),
                width: 100,
                height: 100,
                origin_x: 0,
                origin_y: 0,
                screen_width: 100,
                screen_height: 100,
                is_primary: false,
                kind: TargetKind::Window,
            },
            CaptureTarget {
                id: 2,
                name: "Monitor secundario".to_string(),
                width: 100,
                height: 100,
                origin_x: 0,
                origin_y: 0,
                screen_width: 100,
                screen_height: 100,
                is_primary: false,
                kind: TargetKind::Monitor,
            },
            CaptureTarget {
                id: 1,
                name: "Monitor principal".to_string(),
                width: 100,
                height: 100,
                origin_x: 0,
                origin_y: 0,
                screen_width: 100,
                screen_height: 100,
                is_primary: true,
                kind: TargetKind::Monitor,
            },
        ];

        let sorted = sort_targets(targets);

        assert_eq!(sorted[0].kind, TargetKind::Monitor);
        assert!(sorted[0].is_primary);
        assert_eq!(sorted[1].kind, TargetKind::Monitor);
        assert_eq!(sorted[2].kind, TargetKind::Window);
    }
}
