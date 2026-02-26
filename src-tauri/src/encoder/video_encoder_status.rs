use std::sync::{Mutex, OnceLock};

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
