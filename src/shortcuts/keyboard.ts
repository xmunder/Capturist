export type ShortcutAction = "start" | "pauseResume" | "stop";
export const GLOBAL_SHORTCUT_TRIGGERED_EVENT = "global-shortcut-triggered";

export interface RecorderShortcuts {
  start: string;
  pauseResume: string;
  stop: string;
}

export const DEFAULT_SHORTCUTS: RecorderShortcuts = {
  start: "Ctrl+Alt+R",
  pauseResume: "Ctrl+Alt+P",
  stop: "Ctrl+Alt+S",
};

export const SHORTCUT_LABELS: Record<ShortcutAction, string> = {
  start: "Iniciar",
  pauseResume: "Pausar/Reanudar",
  stop: "Detener",
};

const MODIFIER_ALIASES: Record<string, "Ctrl" | "Alt" | "Shift" | "Meta"> = {
  ctrl: "Ctrl",
  control: "Ctrl",
  alt: "Alt",
  option: "Alt",
  shift: "Shift",
  meta: "Meta",
  cmd: "Meta",
  command: "Meta",
  win: "Meta",
  super: "Meta",
};

const KEY_ALIASES: Record<string, string> = {
  " ": "Space",
  space: "Space",
  spacebar: "Space",
  esc: "Escape",
  escape: "Escape",
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  backspace: "Backspace",
  delete: "Delete",
  del: "Delete",
  insert: "Insert",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  arrowup: "ArrowUp",
  arrowdown: "ArrowDown",
  arrowleft: "ArrowLeft",
  arrowright: "ArrowRight",
  up: "ArrowUp",
  down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
};

export interface KeyboardLikeEvent {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export function normalizeShortcut(value: string) {
  const tokens = value
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean);

  if (tokens.length === 0) {
    return null;
  }

  const modifiers = new Set<"Ctrl" | "Alt" | "Shift" | "Meta">();
  let key: string | null = null;

  for (const token of tokens) {
    const lower = token.toLowerCase();
    const modifier = MODIFIER_ALIASES[lower];
    if (modifier) {
      modifiers.add(modifier);
      continue;
    }

    key = normalizeKeyToken(token);
  }

  if (!key) {
    return null;
  }

  return composeShortcut(modifiers, key);
}

export function buildShortcutFromKeyboardEvent(event: KeyboardLikeEvent) {
  const key = normalizeKeyToken(event.key);
  if (!key || isModifierOnlyKey(key)) {
    return null;
  }

  const modifiers = new Set<"Ctrl" | "Alt" | "Shift" | "Meta">();
  if (event.ctrlKey) modifiers.add("Ctrl");
  if (event.altKey) modifiers.add("Alt");
  if (event.shiftKey) modifiers.add("Shift");
  if (event.metaKey) modifiers.add("Meta");
  return composeShortcut(modifiers, key);
}

export function isEditableTarget(target: EventTarget | null) {
  const element = target as HTMLElement | null;
  if (!element) {
    return false;
  }

  if (element.closest("[data-shortcut-input='true']")) {
    return true;
  }

  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    return !element.readOnly && !element.disabled;
  }

  if (element instanceof HTMLSelectElement) {
    return !element.disabled;
  }

  if (element instanceof HTMLElement && element.isContentEditable) {
    return true;
  }

  return false;
}

export function hydrateShortcuts(raw: unknown) {
  if (!raw || typeof raw !== "object") {
    return { ...DEFAULT_SHORTCUTS };
  }

  const data = raw as Partial<Record<ShortcutAction, unknown>>;
  const start = normalizeShortcut(typeof data.start === "string" ? data.start : "");
  const pauseResume = normalizeShortcut(
    typeof data.pauseResume === "string" ? data.pauseResume : "",
  );
  const stop = normalizeShortcut(typeof data.stop === "string" ? data.stop : "");

  const hydrated: RecorderShortcuts = {
    start: start ?? DEFAULT_SHORTCUTS.start,
    pauseResume: pauseResume ?? DEFAULT_SHORTCUTS.pauseResume,
    stop: stop ?? DEFAULT_SHORTCUTS.stop,
  };

  return ensureUniqueShortcuts(hydrated);
}

function ensureUniqueShortcuts(shortcuts: RecorderShortcuts): RecorderShortcuts {
  const orderedActions: ShortcutAction[] = ["start", "pauseResume", "stop"];
  const normalized: RecorderShortcuts = { ...shortcuts };
  const used = new Set<string>();

  for (const action of orderedActions) {
    const current = normalizeShortcut(normalized[action]) ?? DEFAULT_SHORTCUTS[action];
    if (!used.has(current)) {
      normalized[action] = current;
      used.add(current);
      continue;
    }

    const defaultValue = DEFAULT_SHORTCUTS[action];
    if (!used.has(defaultValue)) {
      normalized[action] = defaultValue;
      used.add(defaultValue);
      continue;
    }

    const firstAvailableDefault = orderedActions
      .map((candidateAction) => DEFAULT_SHORTCUTS[candidateAction])
      .find((candidate) => !used.has(candidate));
    if (firstAvailableDefault) {
      normalized[action] = firstAvailableDefault;
      used.add(firstAvailableDefault);
      continue;
    }

    normalized[action] = current;
    used.add(current);
  }

  return normalized;
}

function composeShortcut(modifiers: Set<"Ctrl" | "Alt" | "Shift" | "Meta">, key: string) {
  const ordered = ["Ctrl", "Alt", "Shift", "Meta"].filter((modifier) =>
    modifiers.has(modifier as "Ctrl" | "Alt" | "Shift" | "Meta"),
  );
  return [...ordered, key].join("+");
}

function normalizeKeyToken(raw: string) {
  if (!raw) {
    return null;
  }

  const trimmed = raw.trim();
  if (!trimmed) {
    return null;
  }

  const lower = trimmed.toLowerCase();
  if (MODIFIER_ALIASES[lower]) {
    return MODIFIER_ALIASES[lower];
  }

  const alias = KEY_ALIASES[lower];
  if (alias) {
    return alias;
  }

  if (trimmed.length === 1) {
    return trimmed.toUpperCase();
  }

  if (/^f\d{1,2}$/i.test(trimmed)) {
    return trimmed.toUpperCase();
  }

  return trimmed[0].toUpperCase() + trimmed.slice(1);
}

function isModifierOnlyKey(key: string) {
  return key === "Ctrl" || key === "Alt" || key === "Shift" || key === "Meta";
}
