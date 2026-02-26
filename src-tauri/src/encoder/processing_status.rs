#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn processing_counter() -> &'static AtomicUsize {
    static PROCESSING_COUNTER: AtomicUsize = AtomicUsize::new(0);
    &PROCESSING_COUNTER
}

fn processing_override_flag() -> &'static AtomicBool {
    static PROCESSING_OVERRIDE_FLAG: AtomicBool = AtomicBool::new(false);
    &PROCESSING_OVERRIDE_FLAG
}

pub struct ProcessingGuard;

impl ProcessingGuard {
    pub fn start() -> Self {
        processing_counter().fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        processing_counter().fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn is_processing() -> bool {
    processing_override_flag().load(Ordering::SeqCst)
        || processing_counter().load(Ordering::SeqCst) > 0
}

pub fn set_processing(value: bool) {
    processing_override_flag().store(value, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::{is_processing, set_processing, ProcessingGuard};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn processing_guard_y_override_controlan_estado() {
        let _guard = test_lock().lock().expect("lock de test poisoned");

        set_processing(false);
        assert!(!is_processing());

        {
            let _processing_guard = ProcessingGuard::start();
            assert!(is_processing());
        }
        assert!(!is_processing());

        set_processing(true);
        assert!(is_processing());
        set_processing(false);
        assert!(!is_processing());
    }
}
