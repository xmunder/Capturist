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

#[derive(Debug, Clone)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct RawFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: u64,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl RawFrame {
    pub fn new(data: Vec<u8>, width: u32, height: u32, timestamp_ms: u64) -> Self {
        Self {
            data,
            width,
            height,
            timestamp_ms,
        }
    }

    pub fn expected_size(width: u32, height: u32) -> usize {
        (width * height * 4) as usize
    }

    pub fn is_valid(&self) -> bool {
        self.data.len() == Self::expected_size(self.width, self.height)
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
