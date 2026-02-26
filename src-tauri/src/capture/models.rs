use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Monitor,
    Window,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub is_primary: bool,
    pub kind: TargetKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    pub fn validate_against_target(&self, target: &CaptureTarget) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("La región de captura debe tener ancho y alto mayores a 0".to_string());
        }

        if self.x + self.width > target.width {
            return Err(format!(
                "La región excede el ancho del target: x({}) + width({}) > target_width({})",
                self.x, self.width, target.width
            ));
        }

        if self.y + self.height > target.height {
            return Err(format!(
                "La región excede el alto del target: y({}) + height({}) > target_height({})",
                self.y, self.height, target.height
            ));
        }

        Ok(())
    }
}

#[derive(Debug)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct RawFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub row_stride_bytes: u32,
    pub gpu_texture_ptr: Option<usize>,
    pub timestamp_ms: u64,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl RawFrame {
    pub fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        row_stride_bytes: u32,
        timestamp_ms: u64,
    ) -> Self {
        let min_row_stride = Self::min_row_stride_bytes(width);
        Self {
            data,
            width,
            height,
            row_stride_bytes: row_stride_bytes.max(min_row_stride),
            gpu_texture_ptr: None,
            timestamp_ms,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn from_gpu_texture(
        width: u32,
        height: u32,
        texture_ptr: usize,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            data: Vec::new(),
            width,
            height,
            row_stride_bytes: 0,
            gpu_texture_ptr: (texture_ptr != 0).then_some(texture_ptr),
            timestamp_ms,
        }
    }

    pub fn min_row_stride_bytes(width: u32) -> u32 {
        width.saturating_mul(4)
    }

    pub fn expected_size(height: u32, row_stride_bytes: u32) -> usize {
        height.saturating_mul(row_stride_bytes) as usize
    }

    pub fn has_cpu_data(&self) -> bool {
        !self.data.is_empty()
    }

    pub fn has_gpu_texture(&self) -> bool {
        self.gpu_texture_ptr.is_some()
    }

    pub fn take_gpu_texture_ptr(&mut self) -> Option<usize> {
        self.gpu_texture_ptr.take()
    }

    pub fn is_valid(&self) -> bool {
        if self.has_cpu_data() {
            if self.width == 0 || self.height == 0 {
                return false;
            }

            if self.row_stride_bytes < Self::min_row_stride_bytes(self.width) {
                return false;
            }

            return self.data.len() >= Self::expected_size(self.height, self.row_stride_bytes);
        }

        if self.has_gpu_texture() {
            return self.width > 0 && self.height > 0;
        }

        false
    }
}

impl Drop for RawFrame {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(ptr) = self.gpu_texture_ptr.take() {
            // El ownership del puntero COM se transfiere en `from_gpu_texture`
            // y se libera de forma segura al descartar el frame.
            unsafe { release_d3d11_texture_ptr(ptr) };
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn release_d3d11_texture_ptr(texture_ptr: usize) {
    use windows::{core::Interface, Win32::Graphics::Direct3D11::ID3D11Texture2D};

    if texture_ptr == 0 {
        return;
    }

    let _ = ID3D11Texture2D::from_raw(texture_ptr as *mut _);
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl RawFrame {
    pub fn is_cpu_layout_valid(&self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }

        if self.row_stride_bytes < Self::min_row_stride_bytes(self.width) {
            return false;
        }

        self.data.len() >= Self::expected_size(self.height, self.row_stride_bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureResolutionPreset {
    Captured,
    #[serde(rename = "480p")]
    R480p,
    #[serde(rename = "720p")]
    R720p,
    #[serde(rename = "1080p")]
    R1080p,
    #[serde(rename = "1440p")]
    R1440p,
    #[serde(rename = "2160p")]
    R2160p,
    #[serde(rename = "4320p")]
    R4320p,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState {
    Idle,
    Running,
    Paused,
    Stopped,
}

impl CaptureState {
    pub fn can_start(&self) -> bool {
        matches!(self, CaptureState::Idle)
    }

    pub fn can_pause(&self) -> bool {
        matches!(self, CaptureState::Running)
    }

    pub fn can_resume(&self) -> bool {
        matches!(self, CaptureState::Paused)
    }

    pub fn can_stop(&self) -> bool {
        matches!(self, CaptureState::Running | CaptureState::Paused)
    }
}

impl std::fmt::Display for CaptureState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureState::Idle => write!(f, "Idle"),
            CaptureState::Running => write!(f, "Running"),
            CaptureState::Paused => write!(f, "Paused"),
            CaptureState::Stopped => write!(f, "Stopped"),
        }
    }
}
