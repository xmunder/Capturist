use std::{collections::HashSet, sync::Mutex};

use tauri::AppHandle;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    pub start: String,
    pub pause_resume: String,
    pub stop: String,
}

pub struct GlobalShortcutManager {
    bindings: Mutex<Option<ShortcutBindings>>,
    _app: AppHandle,
}

impl GlobalShortcutManager {
    pub fn new(app: AppHandle) -> Result<Self, String> {
        Ok(Self {
            bindings: Mutex::new(None),
            _app: app,
        })
    }

    pub fn update(&self, bindings: ShortcutBindings) -> Result<(), String> {
        validate_bindings(&bindings)?;

        let mut guard = self
            .bindings
            .lock()
            .map_err(|_| "No se pudo guardar la configuración de atajos".to_string())?;

        *guard = Some(bindings);
        Ok(())
    }
}

fn validate_bindings(bindings: &ShortcutBindings) -> Result<(), String> {
    let shortcuts = [
        bindings.start.trim(),
        bindings.pause_resume.trim(),
        bindings.stop.trim(),
    ];

    if shortcuts.iter().any(|value| value.is_empty()) {
        return Err("Todos los atajos deben tener una combinación válida".to_string());
    }

    let mut dedup = HashSet::new();
    for value in shortcuts {
        let normalized = value.to_ascii_lowercase();
        if !dedup.insert(normalized) {
            return Err("Cada acción debe tener un atajo distinto".to_string());
        }
    }

    Ok(())
}
