use std::sync::atomic::{AtomicBool, Ordering};

fn processing_flag() -> &'static AtomicBool {
    static PROCESSING_FLAG: AtomicBool = AtomicBool::new(false);
    &PROCESSING_FLAG
}

pub fn is_processing() -> bool {
    processing_flag().load(Ordering::Relaxed)
}

pub fn set_processing(value: bool) {
    processing_flag().store(value, Ordering::Relaxed);
}
