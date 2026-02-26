use std::sync::{Mutex, OnceLock};

use crate::encoder::config::VideoEncoderPreference;

fn video_encoder_label() -> &'static Mutex<Option<String>> {
    static VIDEO_ENCODER_LABEL: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    VIDEO_ENCODER_LABEL.get_or_init(|| Mutex::new(None))
}

pub fn get_live_video_encoder_label() -> Option<String> {
    video_encoder_label()
        .lock()
        .ok()
        .and_then(|value| value.clone())
}

pub fn set_live_video_encoder_label(label: Option<String>) {
    if let Ok(mut guard) = video_encoder_label().lock() {
        *guard = label;
    }
}

pub fn infer_label_from_preference(preference: &VideoEncoderPreference) -> Option<String> {
    match preference {
        VideoEncoderPreference::Auto => None,
        VideoEncoderPreference::Nvenc => Some("NVENC (NVIDIA)".to_string()),
        VideoEncoderPreference::Amf => Some("AMF (AMD)".to_string()),
        VideoEncoderPreference::Qsv => Some("QSV (Intel)".to_string()),
        VideoEncoderPreference::Software => Some("Software (CPU)".to_string()),
    }
}
