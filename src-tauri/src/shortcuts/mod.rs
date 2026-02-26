use tauri::AppHandle;

#[cfg(windows)]
pub const EVENT_GLOBAL_SHORTCUT_TRIGGERED: &str = "global-shortcut-triggered";

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    pub start: String,
    pub pause_resume: String,
    pub stop: String,
}

pub struct GlobalShortcutManager {
    tx: std::sync::mpsc::Sender<PlatformCommand>,
}

impl GlobalShortcutManager {
    pub fn new(app: AppHandle) -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel::<PlatformCommand>();
        std::thread::Builder::new()
            .name("capturist-global-shortcuts".into())
            .spawn(move || run_hotkey_loop(app, rx))
            .map_err(|err| format!("No se pudo iniciar el hilo de atajos globales: {err}"))?;

        Ok(Self { tx })
    }

    pub fn update(&self, bindings: ShortcutBindings) -> Result<(), String> {
        validate_bindings_shape(&bindings)?;

        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        self.tx
            .send(PlatformCommand::Update(bindings, ack_tx))
            .map_err(|_| "No se pudo enviar la actualización de atajos globales".to_string())?;

        ack_rx
            .recv()
            .map_err(|_| "No se recibió confirmación de atajos globales".to_string())?
    }
}

impl Drop for GlobalShortcutManager {
    fn drop(&mut self) {
        let _ = self.tx.send(PlatformCommand::Shutdown);
    }
}

enum PlatformCommand {
    Update(
        ShortcutBindings,
        std::sync::mpsc::Sender<Result<(), String>>,
    ),
    Shutdown,
}

fn validate_bindings_shape(bindings: &ShortcutBindings) -> Result<(), String> {
    use std::collections::HashSet;

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

#[cfg(windows)]
#[derive(Clone, Copy)]
enum ShortcutAction {
    Start,
    PauseResume,
    Stop,
}

#[cfg(windows)]
impl ShortcutAction {
    fn event_payload(self) -> &'static str {
        match self {
            ShortcutAction::Start => "start",
            ShortcutAction::PauseResume => "pauseResume",
            ShortcutAction::Stop => "stop",
        }
    }

    fn index(self) -> usize {
        match self {
            ShortcutAction::Start => 0,
            ShortcutAction::PauseResume => 1,
            ShortcutAction::Stop => 2,
        }
    }
}

#[cfg(windows)]
fn run_hotkey_loop(app: AppHandle, rx: std::sync::mpsc::Receiver<PlatformCommand>) {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    use tauri::Emitter;

    const TRIGGER_COOLDOWN_MS: u64 = 220;

    let mut bindings: Vec<ParsedBinding> = Vec::new();
    let mut pressed_state = [false; 3];
    let mut last_trigger_at = [None::<Instant>; 3];

    loop {
        while let Ok(command) = rx.try_recv() {
            match command {
                PlatformCommand::Update(new_bindings, ack) => {
                    let result = parse_bindings(&new_bindings);
                    match result {
                        Ok(parsed_bindings) => {
                            bindings = parsed_bindings;
                            pressed_state = [false; 3];
                            last_trigger_at = [None, None, None];
                            let _ = ack.send(Ok(()));
                        }
                        Err(err) => {
                            let _ = ack.send(Err(err));
                        }
                    }
                }
                PlatformCommand::Shutdown => {
                    return;
                }
            }
        }

        for binding in &bindings {
            let index = binding.action.index();
            let shortcut_state = read_shortcut_state(&binding.shortcut);
            let combo_down = shortcut_state.combo_down;
            let combo_just_pressed = shortcut_state.combo_just_pressed;
            let was_down = pressed_state[index];

            if (combo_just_pressed || (combo_down && !was_down))
                && can_emit_now(last_trigger_at[index], TRIGGER_COOLDOWN_MS)
            {
                if app
                    .emit(
                        EVENT_GLOBAL_SHORTCUT_TRIGGERED,
                        binding.action.event_payload(),
                    )
                    .is_ok()
                {
                    last_trigger_at[index] = Some(Instant::now());
                }
            }
            pressed_state[index] = combo_down;
        }

        thread::sleep(Duration::from_millis(3));
    }
}

#[cfg(not(windows))]
fn run_hotkey_loop(_app: AppHandle, rx: std::sync::mpsc::Receiver<PlatformCommand>) {
    while let Ok(command) = rx.recv() {
        match command {
            PlatformCommand::Update(bindings, ack) => {
                let _ = bindings;
                let _ = ack.send(Err(
                    "Atajos globales nativos solo disponibles en Windows.".to_string()
                ));
            }
            PlatformCommand::Shutdown => return,
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct ParsedBinding {
    action: ShortcutAction,
    shortcut: ParsedShortcut,
}

#[cfg(windows)]
fn parse_bindings(bindings: &ShortcutBindings) -> Result<Vec<ParsedBinding>, String> {
    let entries = [
        (ShortcutAction::Start, bindings.start.as_str()),
        (ShortcutAction::PauseResume, bindings.pause_resume.as_str()),
        (ShortcutAction::Stop, bindings.stop.as_str()),
    ];

    let mut parsed_bindings = Vec::with_capacity(entries.len());

    for (action, shortcut) in entries {
        let parsed = parse_shortcut(shortcut)?;
        if parsed_bindings
            .iter()
            .any(|current: &ParsedBinding| current.shortcut == parsed)
        {
            return Err(format!(
                "El atajo '{shortcut}' está duplicado. Cada acción debe tener una combinación distinta."
            ));
        }

        parsed_bindings.push(ParsedBinding {
            action,
            shortcut: parsed,
        });
    }

    Ok(parsed_bindings)
}

#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct ParsedShortcut {
    modifiers: u32,
    vk: u32,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct ShortcutReadState {
    combo_down: bool,
    combo_just_pressed: bool,
}

#[cfg(windows)]
fn read_shortcut_state(shortcut: &ParsedShortcut) -> ShortcutReadState {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
        VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    };

    let main_key_state = query_key_state(shortcut.vk as i32);
    let main_key_down = is_key_state_down(main_key_state);
    let main_key_pressed_since_last_poll = was_key_pressed_since_last_poll(main_key_state);

    if !main_key_down && !main_key_pressed_since_last_poll {
        return ShortcutReadState {
            combo_down: false,
            combo_just_pressed: false,
        };
    }

    if (shortcut.modifiers & MOD_CONTROL) != 0
        && !is_any_key_down(&[VK_CONTROL as i32, VK_LCONTROL as i32, VK_RCONTROL as i32])
    {
        return ShortcutReadState {
            combo_down: false,
            combo_just_pressed: false,
        };
    }

    if (shortcut.modifiers & MOD_ALT) != 0
        && !is_any_key_down(&[VK_MENU as i32, VK_LMENU as i32, VK_RMENU as i32])
    {
        return ShortcutReadState {
            combo_down: false,
            combo_just_pressed: false,
        };
    }

    if (shortcut.modifiers & MOD_SHIFT) != 0
        && !is_any_key_down(&[VK_SHIFT as i32, VK_LSHIFT as i32, VK_RSHIFT as i32])
    {
        return ShortcutReadState {
            combo_down: false,
            combo_just_pressed: false,
        };
    }

    if (shortcut.modifiers & MOD_WIN) != 0 && !is_any_key_down(&[VK_LWIN as i32, VK_RWIN as i32]) {
        return ShortcutReadState {
            combo_down: false,
            combo_just_pressed: false,
        };
    }

    ShortcutReadState {
        combo_down: main_key_down,
        combo_just_pressed: main_key_pressed_since_last_poll,
    }
}

#[cfg(windows)]
fn is_any_key_down(vks: &[i32]) -> bool {
    vks.iter().any(|vk| is_key_down(*vk))
}

#[cfg(windows)]
fn can_emit_now(last_emit_at: Option<std::time::Instant>, cooldown_ms: u64) -> bool {
    match last_emit_at {
        Some(instant) => instant.elapsed().as_millis() >= cooldown_ms as u128,
        None => true,
    }
}

#[cfg(windows)]
fn is_key_down(vk: i32) -> bool {
    is_key_state_down(query_key_state(vk))
}

#[cfg(windows)]
fn is_key_state_down(state: u16) -> bool {
    state & 0x8000 != 0
}

#[cfg(windows)]
fn was_key_pressed_since_last_poll(state: u16) -> bool {
    state & 0x0001 != 0
}

#[cfg(windows)]
fn query_key_state(vk: i32) -> u16 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    // SAFETY: consulta global de estado de teclado desde Win32.
    unsafe { GetAsyncKeyState(vk) as u16 }
}

#[cfg(windows)]
fn parse_shortcut(value: &str) -> Result<ParsedShortcut, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
    };

    let tokens: Vec<&str> = value
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect();

    if tokens.is_empty() {
        return Err("Atajo vacío".to_string());
    }

    let mut modifiers = 0_u32;
    let mut key: Option<u32> = None;

    for token in tokens {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "meta" | "win" | "super" | "cmd" | "command" => modifiers |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return Err(format!(
                        "Atajo inválido '{value}'. Solo puede haber una tecla principal."
                    ));
                }
                key = Some(parse_virtual_key(token)?);
            }
        }
    }

    let vk = key.ok_or_else(|| format!("Atajo inválido '{value}'. Falta la tecla principal."))?;
    Ok(ParsedShortcut { modifiers, vk })
}

#[cfg(windows)]
fn parse_virtual_key(token: &str) -> Result<u32, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT, VK_LEFT,
        VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
    };

    let trimmed = token.trim();
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap_or_default();
        if ch.is_ascii_alphabetic() {
            return Ok(ch.to_ascii_uppercase() as u32);
        }
        if ch.is_ascii_digit() {
            return Ok(ch as u32);
        }
    }

    let upper = trimmed.to_ascii_uppercase();
    let vk = match upper.as_str() {
        "SPACE" | "SPACEBAR" => VK_SPACE as u32,
        "ENTER" | "RETURN" => VK_RETURN as u32,
        "TAB" => VK_TAB as u32,
        "ESC" | "ESCAPE" => VK_ESCAPE as u32,
        "BACKSPACE" => VK_BACK as u32,
        "DELETE" | "DEL" => VK_DELETE as u32,
        "INSERT" => VK_INSERT as u32,
        "HOME" => VK_HOME as u32,
        "END" => VK_END as u32,
        "PAGEUP" => VK_PRIOR as u32,
        "PAGEDOWN" => VK_NEXT as u32,
        "UP" | "ARROWUP" => VK_UP as u32,
        "DOWN" | "ARROWDOWN" => VK_DOWN as u32,
        "LEFT" | "ARROWLEFT" => VK_LEFT as u32,
        "RIGHT" | "ARROWRIGHT" => VK_RIGHT as u32,
        _ => {
            if let Some(rest) = upper.strip_prefix('F') {
                if let Ok(number) = rest.parse::<u32>() {
                    if (1..=24).contains(&number) {
                        return Ok(VK_F1 as u32 + (number - 1));
                    }
                }
            }
            return Err(format!("Tecla no soportada en atajo: '{token}'"));
        }
    };

    Ok(vk)
}

#[cfg(test)]
mod tests {
    use super::{validate_bindings_shape, ShortcutBindings};

    #[test]
    fn valida_atajos_distintos_y_no_vacios() {
        let bindings = ShortcutBindings {
            start: "Ctrl+Alt+R".to_string(),
            pause_resume: "Ctrl+Alt+P".to_string(),
            stop: "Ctrl+Alt+S".to_string(),
        };

        assert!(validate_bindings_shape(&bindings).is_ok());
    }

    #[test]
    fn rechaza_atajos_vacios() {
        let bindings = ShortcutBindings {
            start: " ".to_string(),
            pause_resume: "Ctrl+Alt+P".to_string(),
            stop: "Ctrl+Alt+S".to_string(),
        };

        let err = validate_bindings_shape(&bindings).expect_err("debio fallar por atajo vacio");
        assert!(err.contains("combinación válida"));
    }

    #[test]
    fn rechaza_atajos_duplicados_sin_importar_mayusculas() {
        let bindings = ShortcutBindings {
            start: "Ctrl+Alt+R".to_string(),
            pause_resume: "ctrl+alt+r".to_string(),
            stop: "Ctrl+Alt+S".to_string(),
        };

        let err =
            validate_bindings_shape(&bindings).expect_err("debio fallar por atajos duplicados");
        assert!(err.contains("atajo distinto"));
    }
}
