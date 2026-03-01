import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { join, homeDir } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { exists, mkdir } from "@tauri-apps/plugin-fs";
import { Grabador } from "../../recorder/Grabador";
import type {
  CaptureManagerSnapshot,
  CaptureState,
  CaptureTarget,
  CropRegion,
  EncoderPreset,
  OutputFormat,
  OutputResolution,
  RecordingQualityMode,
  RecordingAudioStatus,
  RecordingSessionConfig,
  VideoCodec,
  VideoEncoderCapabilities,
  VideoEncoderPreference,
} from "../../recorder/types";
import {
  DEFAULT_SHORTCUTS,
  GLOBAL_SHORTCUT_TRIGGERED_EVENT,
  SHORTCUT_LABELS,
  buildShortcutFromKeyboardEvent,
  hydrateShortcuts,
  isEditableTarget,
  normalizeShortcut,
  type RecorderShortcuts,
  type ShortcutAction,
} from "../../shortcuts/keyboard";
import {
  DEFAULT_CRF,
  DEFAULT_OUTPUT_NAME,
  STATUS_LABELS,
  type CodecChoice,
  defaultVideosDir,
  formatCodec,
  formatElapsed,
  injectOutputNameTokens,
  stripExtension,
  toCompactPath,
  withExtension,
} from "./utils";

type ResolutionChoice = "fullHd" | "hd" | "sd" | "p1440" | "p2160" | "custom";

const RESOLUTION_LABELS: Record<Exclude<ResolutionChoice, "custom">, string> = {
  fullHd: "1920x1080",
  hd: "1280x720",
  sd: "854x480",
  p1440: "2560x1440",
  p2160: "3840x2160",
};

const RESOLUTION_DIMENSIONS: Record<Exclude<ResolutionChoice, "custom">, { width: number; height: number }> = {
  fullHd: { width: 1920, height: 1080 },
  hd: { width: 1280, height: 720 },
  sd: { width: 854, height: 480 },
  p1440: { width: 2560, height: 1440 },
  p2160: { width: 3840, height: 2160 },
};

const DEFAULT_VIDEO_ENCODER_CAPABILITIES: VideoEncoderCapabilities = {
  nvenc: false,
  amf: false,
  qsv: false,
  software: true,
};

const DEBUG_REGION = true;
const SHORTCUTS_STORAGE_KEY = "capturist.shortcuts.v1";
const LEGACY_DEFAULT_SHORTCUTS: RecorderShortcuts = {
  start: "Ctrl+Shift+R",
  pauseResume: "Ctrl+Shift+P",
  stop: "Ctrl+Shift+S",
};

function debugRegion(...args: unknown[]) {
  if (DEBUG_REGION || import.meta.env.DEV) {
    console.log("[region-debug]", ...args);
  }
}

function parsePositive(value: string, fallback: number) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  return Math.round(parsed);
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function resolvePreferredTargetId(targets: CaptureTarget[], preferredId: number | null) {
  if (targets.length === 0) {
    return null;
  }
  if (preferredId != null && targets.some((target) => target.id === preferredId)) {
    return preferredId;
  }
  const primary = targets.find((target) => target.isPrimary) ?? targets[0];
  return primary?.id ?? null;
}

function resolveCodecSelection(choice: CodecChoice): {
  codec: VideoCodec | null;
  videoEncoderPreference: VideoEncoderPreference;
} {
  switch (choice) {
    case "auto":
      return { codec: null, videoEncoderPreference: "auto" };
    case "h264":
      return { codec: "h264", videoEncoderPreference: "auto" };
    case "h265":
      return { codec: "h265", videoEncoderPreference: "auto" };
    case "vp9":
      return { codec: "vp9", videoEncoderPreference: "auto" };
    case "nvenc":
      return { codec: "h264", videoEncoderPreference: "nvenc" };
    case "amf":
      return { codec: "h264", videoEncoderPreference: "amf" };
    case "qsv":
      return { codec: "h264", videoEncoderPreference: "qsv" };
    default:
      return { codec: null, videoEncoderPreference: "auto" };
  }
}

function isCodecChoiceAvailable(choice: CodecChoice, capabilities: VideoEncoderCapabilities): boolean {
  switch (choice) {
    case "nvenc":
      return capabilities.nvenc;
    case "amf":
      return capabilities.amf;
    case "qsv":
      return capabilities.qsv;
    default:
      return true;
  }
}

function loadStoredShortcuts(): RecorderShortcuts {
  if (typeof window === "undefined") {
    return { ...DEFAULT_SHORTCUTS };
  }

  try {
    const raw = window.localStorage.getItem(SHORTCUTS_STORAGE_KEY);
    if (!raw) {
      return { ...DEFAULT_SHORTCUTS };
    }

    const hydrated = hydrateShortcuts(JSON.parse(raw));

    // Migración automática: si el usuario nunca cambió los defaults antiguos,
    // aplicar los nuevos defaults Ctrl+Alt sin romper atajos personalizados.
    if (
      hydrated.start === LEGACY_DEFAULT_SHORTCUTS.start &&
      hydrated.pauseResume === LEGACY_DEFAULT_SHORTCUTS.pauseResume &&
      hydrated.stop === LEGACY_DEFAULT_SHORTCUTS.stop
    ) {
      return { ...DEFAULT_SHORTCUTS };
    }

    return hydrated;
  } catch {
    return { ...DEFAULT_SHORTCUTS };
  }
}

export function useRecorderController() {
  const [supported, setSupported] = useState<boolean | null>(null);
  const [targets, setTargets] = useState<CaptureTarget[]>([]);
  const [selectedTargetId, setSelectedTargetId] = useState<number | null>(null);
  const [status, setStatus] = useState<CaptureState>("idle");
  const [isProcessing, setIsProcessing] = useState(false);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [lastError, setLastError] = useState<string | null>(null);
  const [activeVideoEncoderLabel, setActiveVideoEncoderLabel] = useState<string | null>(null);
  const [videoEncoderCapabilities, setVideoEncoderCapabilities] = useState<VideoEncoderCapabilities>(
    DEFAULT_VIDEO_ENCODER_CAPABILITIES,
  );

  const [outputDir, setOutputDir] = useState("");
  const [homePath, setHomePath] = useState("");
  const [outputName, setOutputName] = useState(DEFAULT_OUTPUT_NAME);
  const [format, setFormat] = useState<OutputFormat>("mp4");
  const [codec, setCodec] = useState<CodecChoice>("auto");
  const [preset, setPreset] = useState<EncoderPreset>("ultraFast");
  const [qualityMode, setQualityMode] = useState<RecordingQualityMode>("balanced");
  const [resolutionChoice, setResolutionChoice] = useState<ResolutionChoice>("fullHd");
  const [fps, setFps] = useState(30);
  const [crf, setCrf] = useState(DEFAULT_CRF);
  const [customWidth, setCustomWidth] = useState(1920);
  const [customHeight, setCustomHeight] = useState(1080);
  const [captureSystemAudio, setCaptureSystemAudioState] = useState(false);
  const [captureMicrophoneAudio, setCaptureMicrophoneAudioState] = useState(false);
  const [microphoneGainPercent, setMicrophoneGainPercentState] = useState(100);
  const [recordingAudioStatus, setRecordingAudioStatus] = useState<RecordingAudioStatus>({
    captureSystemAudio: false,
    captureMicrophoneAudio: false,
    systemAudioDeviceName: null,
    microphoneAudioDeviceName: null,
  });
  const [audioInputDevices, setAudioInputDevices] = useState<string[]>([]);
  const [selectedMicrophoneDevice, setSelectedMicrophoneDevice] = useState<string | null>(null);
  const [keyboardShortcuts, setKeyboardShortcuts] =
    useState<RecorderShortcuts>(loadStoredShortcuts);

  const [cropEnabled, setCropEnabled] = useState(false);
  const [cropRegion, setCropRegion] = useState<CropRegion>({
    x: 0,
    y: 0,
    width: 1280,
    height: 720,
  });

  const [busy, setBusy] = useState(false);
  const [errorMsg, setErrorMsg] = useState("");
  const [globalShortcutsEnabled, setGlobalShortcutsEnabled] = useState(false);

  const activeTarget = useMemo(
    () => targets.find((target) => target.id === selectedTargetId) ?? null,
    [targets, selectedTargetId],
  );

  const isRecording = status === "running" || status === "paused";

  const supports4kOutput = useMemo(() => {
    if (!activeTarget || activeTarget.kind !== "monitor") {
      return false;
    }
    const maxDim = Math.max(activeTarget.width, activeTarget.height);
    const minDim = Math.min(activeTarget.width, activeTarget.height);
    return maxDim >= 3840 && minDim >= 2160;
  }, [activeTarget]);

  const applyMicrophoneDeviceList = (devices: string[]) => {
    setAudioInputDevices(devices);
    setSelectedMicrophoneDevice((current) => {
      if (current && devices.includes(current)) {
        return current;
      }
      return devices[0] ?? null;
    });
  };

  const reloadTargetsKeepingSelection = async (preferredId = selectedTargetId) => {
    const nextTargets = await Grabador.getTargets();
    const nextSelectedId = resolvePreferredTargetId(nextTargets, preferredId);
    setTargets(nextTargets);
    setSelectedTargetId(nextSelectedId);
    return {
      targets: nextTargets,
      selectedId: nextSelectedId,
    };
  };

  const compactOutputDir = useMemo(
    () => toCompactPath(outputDir, homePath) || "Sin definir",
    [outputDir, homePath],
  );

  const outputResolutionLabel =
    resolutionChoice === "custom"
      ? `${customWidth}x${customHeight}`
      : RESOLUTION_LABELS[resolutionChoice];
  const elapsedLabel = formatElapsed(elapsedMs);
  const codecLabel = (() => {
    if (activeVideoEncoderLabel) {
      if (codec === "auto") {
        return `Auto (${activeVideoEncoderLabel})`;
      }
      return activeVideoEncoderLabel;
    }
    return formatCodec(codec);
  })();
  const statusLabel = STATUS_LABELS[status];
  const effectiveSystemAudioEnabled = isRecording
    ? recordingAudioStatus.captureSystemAudio
    : captureSystemAudio;
  const effectiveMicrophoneAudioEnabled = isRecording
    ? recordingAudioStatus.captureMicrophoneAudio
    : captureMicrophoneAudio;
  const audioActiveLabel = effectiveSystemAudioEnabled
    ? effectiveMicrophoneAudioEnabled
      ? "Equipo + micrófono"
      : "Equipo"
    : effectiveMicrophoneAudioEnabled
      ? "Micrófono"
      : "Desactivado";
  const activeSystemDeviceName =
    recordingAudioStatus.systemAudioDeviceName?.trim() || "Salida predeterminada del sistema";
  const activeMicrophoneDeviceName =
    recordingAudioStatus.microphoneAudioDeviceName?.trim() ||
    selectedMicrophoneDevice ||
    "Micrófono predeterminado";
  const audioActiveDevicesLabel = effectiveSystemAudioEnabled
    ? effectiveMicrophoneAudioEnabled
      ? `${activeSystemDeviceName} + ${activeMicrophoneDeviceName}`
      : activeSystemDeviceName
    : effectiveMicrophoneAudioEnabled
      ? activeMicrophoneDeviceName
      : "Sin audio activo";
  const captureModeLabel = cropEnabled && activeTarget?.kind === "monitor" ? "Región" : "Completa";
  const systemCaptureLabel = effectiveSystemAudioEnabled ? "Activo" : "Inactivo";
  const microphoneCaptureLabel = effectiveMicrophoneAudioEnabled ? `${microphoneGainPercent}%` : "0%";

  const applyStatusSnapshot = (snapshot: CaptureManagerSnapshot) => {
    setStatus(snapshot.state);
    setElapsedMs(snapshot.elapsedMs);
    setLastError(snapshot.lastError ?? null);
    setActiveVideoEncoderLabel(snapshot.videoEncoderLabel ?? null);
    setIsProcessing(snapshot.isProcessing ?? false);
  };

  useEffect(() => {
    let mounted = true;

    const boot = async () => {
      try {
        const userHome = await homeDir();
        if (!mounted) return;
        setHomePath(userHome);

        const isSupported = await Grabador.isCaptureSupported();
        if (!mounted) return;
        setSupported(isSupported);

        if (isSupported) {
          const list = await Grabador.getTargets();
          if (!mounted) return;

          setTargets(list);
          const primary = list.find((target) => target.isPrimary) ?? list[0];
          setSelectedTargetId(primary?.id ?? null);
        }

        let microphones: string[] = [];
        try {
          microphones = await Grabador.getAudioInputDevices();
        } catch {
          microphones = [];
        }

        let encoderCapabilities = DEFAULT_VIDEO_ENCODER_CAPABILITIES;
        try {
          encoderCapabilities = await Grabador.getVideoEncoderCapabilities();
        } catch {
          encoderCapabilities = DEFAULT_VIDEO_ENCODER_CAPABILITIES;
        }

        if (!mounted) return;
        applyMicrophoneDeviceList(microphones);
        setVideoEncoderCapabilities(encoderCapabilities);

        const preferredDir = await defaultVideosDir(userHome);
        const fallbackDir = await join(userHome, "Videos");
        let baseDir = preferredDir;

        try {
          const hasPreferredDir = await exists(preferredDir);
          if (!hasPreferredDir) {
            try {
              await mkdir(preferredDir, { recursive: true });
            } catch {
              baseDir = fallbackDir;
            }
          }
        } catch {
          baseDir = fallbackDir;
        }

        if (!mounted) return;
        setOutputDir(baseDir);
        setOutputName(DEFAULT_OUTPUT_NAME);
      } catch (err) {
        if (!mounted) return;
        setErrorMsg(String(err));
      }
    };

    void boot();
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const [snapshot, liveAudioStatus] = await Promise.all([
          Grabador.status(),
          Grabador.recordingAudioStatus(),
        ]);
        applyStatusSnapshot(snapshot);
        setRecordingAudioStatus(liveAudioStatus);
      } catch {
        // silent
      }
    }, 500);

    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (format === "webM" && codec !== "vp9") {
      setCodec("vp9");
    }
  }, [codec, format]);

  useEffect(() => {
    if (!isCodecChoiceAvailable(codec, videoEncoderCapabilities)) {
      setCodec("auto");
    }
  }, [codec, videoEncoderCapabilities]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    try {
      window.localStorage.setItem(SHORTCUTS_STORAGE_KEY, JSON.stringify(keyboardShortcuts));
    } catch {
      // silencioso: no bloquear la UI si localStorage falla.
    }
  }, [keyboardShortcuts]);

  useEffect(() => {
    if (activeTarget?.kind === "window" && cropEnabled) {
      setCropEnabled(false);
    }
  }, [activeTarget?.kind, cropEnabled]);

  const resolveOutputResolution = (): OutputResolution => {
    if (resolutionChoice !== "custom") {
      return resolutionChoice;
    }
    return {
      custom: {
        width: Math.max(1, Math.round(customWidth)),
        height: Math.max(1, Math.round(customHeight)),
      },
    };
  };

  const pickOutputDir = async () => {
    try {
      const selection = await open({ directory: true, multiple: false });
      if (typeof selection === "string") {
        setOutputDir(selection);
      }
    } catch (err) {
      setErrorMsg(String(err));
    }
  };

  const buildOutputPath = async () => {
    const ext = format.toLowerCase();
    const rawName = outputName.trim() || DEFAULT_OUTPUT_NAME;
    const namePattern = /\{date\}|\{time\}/.test(rawName) ? rawName : `${rawName}_{date}_{time}`;
    const name = injectOutputNameTokens(namePattern);
    const baseName = stripExtension(name);
    return join(outputDir, `${baseName}.${ext}`);
  };

  const syncLiveAudioCapture = async (
    nextSystemAudio: boolean,
    nextMicrophoneAudio: boolean,
    rollback: () => void,
  ) => {
    const sessionActive = status === "running" || status === "paused";
    if (!sessionActive) {
      return;
    }

    try {
      await Grabador.updateRecordingAudioCapture(nextSystemAudio, nextMicrophoneAudio);
    } catch (err) {
      const firstError = String(err);
      try {
        await new Promise((resolve) => setTimeout(resolve, 120));
        await Grabador.updateRecordingAudioCapture(nextSystemAudio, nextMicrophoneAudio);
      } catch {
        rollback();
        setErrorMsg(firstError);
      }
    }
  };

  const setCaptureSystemAudio = async (enabled: boolean) => {
    const previous = captureSystemAudio;
    setCaptureSystemAudioState(enabled);
    await syncLiveAudioCapture(enabled, captureMicrophoneAudio, () => {
      setCaptureSystemAudioState(previous);
    });
  };

  const setCaptureMicrophoneAudio = async (enabled: boolean) => {
    const previous = captureMicrophoneAudio;
    setCaptureMicrophoneAudioState(enabled);
    await syncLiveAudioCapture(captureSystemAudio, enabled, () => {
      setCaptureMicrophoneAudioState(previous);
    });
  };

  const setMicrophoneGainPercent = (value: number) => {
    if (!Number.isFinite(value)) {
      return;
    }
    setMicrophoneGainPercentState(clamp(Math.round(value), 0, 400));
  };

  const setKeyboardShortcut = (action: ShortcutAction, shortcut: string) => {
    const normalized = normalizeShortcut(shortcut);
    if (!normalized) {
      setErrorMsg("Atajo inválido. Usa una combinación con una tecla.");
      return;
    }

    const duplicate = (Object.entries(keyboardShortcuts) as [ShortcutAction, string][])
      .find(([currentAction, currentShortcut]) => {
        return currentAction !== action && currentShortcut === normalized;
      });

    if (duplicate) {
      setErrorMsg(
        `El atajo ${normalized} ya está asignado a ${SHORTCUT_LABELS[duplicate[0]]}.`,
      );
      return;
    }

    setErrorMsg("");
    setKeyboardShortcuts((current) => ({ ...current, [action]: normalized }));
  };

  const resetKeyboardShortcuts = () => {
    setErrorMsg("");
    setKeyboardShortcuts({ ...DEFAULT_SHORTCUTS });
  };

  const startRecording = async () => {
    if (selectedTargetId == null) {
      setErrorMsg("Selecciona un target de captura.");
      return;
    }

    if (!outputDir.trim()) {
      setErrorMsg("Selecciona una carpeta de salida.");
      return;
    }

    setBusy(true);
    setErrorMsg("");

    const currentWindow = getCurrentWindow();
    let minimizedByStart = false;

    try {
      const selectedTargetIdAtStart = selectedTargetId;
      const { targets: latestTargets } = await reloadTargetsKeepingSelection(
        selectedTargetIdAtStart,
      );
      const selectedTarget = latestTargets.find(
        (target) => target.id === selectedTargetIdAtStart,
      );

      if (!selectedTargetIdAtStart || !selectedTarget) {
        setLastError(null);
        setErrorMsg(
          "La fuente seleccionada ya no existe. Se actualizó la lista; selecciona una fuente y vuelve a intentar.",
        );
        return;
      }

      const outputPath = await buildOutputPath();
      const shouldApplyCrop = cropEnabled && selectedTarget.kind === "monitor";
      const resolvedCodec = resolveCodecSelection(codec);
      const payload: RecordingSessionConfig = {
        targetId: selectedTargetIdAtStart,
        fps,
        cropRegion: shouldApplyCrop ? cropRegion : null,
        outputPath: withExtension(outputPath, format.toLowerCase()),
        format,
        codec: resolvedCodec.codec,
        videoEncoderPreference: resolvedCodec.videoEncoderPreference,
        resolution: resolveOutputResolution(),
        crf,
        preset,
        qualityMode,
        captureSystemAudio,
        captureMicrophoneAudio,
        systemAudioDevice: null,
        microphoneDevice: selectedMicrophoneDevice,
        microphoneGainPercent,
      };

      try {
        const wasMinimized = await currentWindow.isMinimized();
        if (!wasMinimized) {
          await currentWindow.minimize();
          minimizedByStart = true;
          await new Promise((resolve) => setTimeout(resolve, 180));
        }
      } catch (minimizeErr) {
        console.warn("[window] no se pudo minimizar antes de grabar", minimizeErr);
      }

      debugRegion("startRecording payload", payload);
      await Grabador.start(payload);

      const snapshot = await Grabador.status();
      applyStatusSnapshot(snapshot);
    } catch (err) {
      if (minimizedByStart) {
        try {
          await currentWindow.unminimize();
          await currentWindow.show();
          await currentWindow.setFocus();
        } catch {
          // silencioso: evitar ocultar el error original de inicio.
        }
      }
      const message = String(err);
      setErrorMsg(message);
    } finally {
      setBusy(false);
    }
  };

  const pauseRecording = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      await Grabador.pause();
      const snapshot = await Grabador.status();
      applyStatusSnapshot(snapshot);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const resumeRecording = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      await Grabador.resume();
      const snapshot = await Grabador.status();
      applyStatusSnapshot(snapshot);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const stopRecording = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      await Grabador.stop();
      const snapshot = await Grabador.status();
      applyStatusSnapshot(snapshot);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const liveStateRef = useRef({
    busy,
    status,
    supported,
    isProcessing,
  });
  const liveActionsRef = useRef({
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
  });

  useEffect(() => {
    liveStateRef.current = {
      busy,
      status,
      supported,
      isProcessing,
    };
  }, [busy, status, supported, isProcessing]);

  useEffect(() => {
    liveActionsRef.current = {
      startRecording,
      pauseRecording,
      resumeRecording,
      stopRecording,
    };
  }, [pauseRecording, resumeRecording, startRecording, stopRecording]);

  const runShortcutAction = useCallback((action: ShortcutAction) => {
    const current = liveStateRef.current;
    const actions = liveActionsRef.current;
    if (current.busy || current.isProcessing) {
      return;
    }

    if (action === "start") {
      if (current.status === "idle" && Boolean(current.supported)) {
        void actions.startRecording();
      }
      return;
    }

    if (action === "pauseResume") {
      if (current.status === "running") {
        void actions.pauseRecording();
        return;
      }
      if (current.status === "paused") {
        void actions.resumeRecording();
      }
      return;
    }

    if (action === "stop") {
      if (current.status === "running" || current.status === "paused") {
        void actions.stopRecording();
      }
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const bind = async () => {
      unlisten = await listen<string>(GLOBAL_SHORTCUT_TRIGGERED_EVENT, (event) => {
        const action = event.payload;
        if (action === "start" || action === "pauseResume" || action === "stop") {
          runShortcutAction(action);
        }
      });
    };

    void bind();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [runShortcutAction]);

  useEffect(() => {
    let mounted = true;

    const syncGlobalShortcuts = async () => {
      try {
        await invoke("set_global_shortcuts", { config: keyboardShortcuts });
        if (!mounted) return;
        setGlobalShortcutsEnabled(true);
      } catch (err) {
        if (!mounted) return;
        setGlobalShortcutsEnabled(false);
        console.warn("[shortcuts] global shortcuts no disponibles, fallback local", err);
      }
    };

    void syncGlobalShortcuts();
    return () => {
      mounted = false;
    };
  }, [keyboardShortcuts]);

  useEffect(() => {
    if (globalShortcutsEnabled) {
      return;
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat || isEditableTarget(event.target)) {
        return;
      }

      const pressedShortcut = buildShortcutFromKeyboardEvent(event);
      if (!pressedShortcut) {
        return;
      }

      if (pressedShortcut === keyboardShortcuts.start) {
        event.preventDefault();
        runShortcutAction("start");
        return;
      }

      if (pressedShortcut === keyboardShortcuts.pauseResume) {
        event.preventDefault();
        runShortcutAction("pauseResume");
        return;
      }

      if (pressedShortcut === keyboardShortcuts.stop) {
        event.preventDefault();
        runShortcutAction("stop");
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [globalShortcutsEnabled, keyboardShortcuts, runShortcutAction]);

  const cancelRecording = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      await Grabador.cancel();
      const snapshot = await Grabador.status();
      applyStatusSnapshot(snapshot);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const refreshTargets = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      await reloadTargetsKeepingSelection();
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const refreshAudioInputDevices = async () => {
    setBusy(true);
    setErrorMsg("");

    try {
      const microphones = await Grabador.getAudioInputDevices();
      applyMicrophoneDeviceList(microphones);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setBusy(false);
    }
  };

  const openRegionOverlay = async () => {
    if (!activeTarget) {
      setErrorMsg("Selecciona un target primero.");
      return;
    }

    debugRegion("selectRegionNative requested", {
      target: activeTarget,
      cropEnabled,
      cropRegion,
    });

    const currentWindow = getCurrentWindow();
    let shouldRestoreWindow = false;

    try {
      try {
        const wasMinimized = await currentWindow.isMinimized();
        if (!wasMinimized) {
          shouldRestoreWindow = true;
          await currentWindow.minimize();
          await new Promise((resolve) => setTimeout(resolve, 180));
        }
      } catch (minimizeErr) {
        shouldRestoreWindow = false;
        console.warn("[window] no se pudo minimizar antes de seleccionar region", minimizeErr);
      }

      const region = await Grabador.selectRegionNative(activeTarget);
      debugRegion("selectRegionNative result", region);

      if (!region) {
        return;
      }

      setCropRegion(region);
      setCropEnabled(true);
    } catch (err) {
      const message = String(err);
      debugRegion("selectRegionNative error", message);
      setErrorMsg(`Error seleccionando región nativa: ${message}`);
    } finally {
      if (shouldRestoreWindow) {
        try {
          await currentWindow.unminimize();
          await currentWindow.show();
          await currentWindow.setFocus();
        } catch {
          // silencioso: evitar bloquear UX si el sistema rechaza foco.
        }
      }
    }
  };

  const setCustomWidthFromInput = (value: string) => {
    setCustomWidth((current) => parsePositive(value, current));
  };

  const setCustomHeightFromInput = (value: string) => {
    setCustomHeight((current) => parsePositive(value, current));
  };

  const setResolutionChoiceWithDims = (choice: ResolutionChoice) => {
    setResolutionChoice(choice);
    if (choice !== "custom") {
      const dims = RESOLUTION_DIMENSIONS[choice];
      setCustomWidth(dims.width);
      setCustomHeight(dims.height);
    }
  };

  useEffect(() => {
    if (!supports4kOutput && resolutionChoice === "p2160") {
      setResolutionChoiceWithDims("p1440");
    }
  }, [resolutionChoice, supports4kOutput]);

  return {
    state: {
      supported,
      targets,
      selectedTargetId,
      status,
      elapsedMs,
      lastError,
      outputDir,
      outputName,
      format,
      codec,
      preset,
      qualityMode,
      resolutionChoice,
      fps,
      crf,
      customWidth,
      customHeight,
      captureSystemAudio,
      captureMicrophoneAudio,
      microphoneGainPercent,
      audioInputDevices,
      selectedMicrophoneDevice,
      keyboardShortcuts,
      cropEnabled,
      cropRegion,
      busy,
      errorMsg,
    },
    derived: {
      activeTarget,
      isRecording,
      supports4kOutput,
      isProcessing,
      compactOutputDir,
      outputResolutionLabel,
      elapsedLabel,
      codecLabel,
      videoEncoderCapabilities,
      statusLabel,
      showCropControls: activeTarget?.kind !== "window",
      hasAlerts: Boolean(lastError || errorMsg),
      audioActiveLabel,
      audioActiveDevicesLabel,
      captureModeLabel,
      systemCaptureLabel,
      microphoneCaptureLabel,
    },
    actions: {
      setSelectedTargetId,
      setCropEnabled,
      setFormat,
      setCodec,
      setPreset,
      setQualityMode,
      setResolutionChoice: setResolutionChoiceWithDims,
      setFps,
      setCrf,
      setOutputDir,
      setOutputName,
      setCustomWidthFromInput,
      setCustomHeightFromInput,
      setCaptureSystemAudio,
      setCaptureMicrophoneAudio,
      setMicrophoneGainPercent,
      setKeyboardShortcut,
      resetKeyboardShortcuts,
      setSelectedMicrophoneDevice,
      pickOutputDir,
      refreshTargets,
      refreshAudioInputDevices,
      openRegionOverlay,
      startRecording,
      pauseRecording,
      resumeRecording,
      stopRecording,
      cancelRecording,
    },
  };
}

export type UseRecorderControllerReturn = ReturnType<typeof useRecorderController>;
