import { useCallback, useEffect, useRef, useState } from "react";
import type { EncoderPreset, OutputFormat, RecordingQualityMode } from "../../recorder/types";
import type { CodecChoice } from "../../hooks/useRecorderController";
import {
  SHORTCUT_LABELS,
  buildShortcutFromKeyboardEvent,
  type RecorderShortcuts,
  type ShortcutAction,
} from "../../shortcuts/keyboard";
import styles from "./AdvancedSettingsModal.module.css";

interface FieldLabelProps {
  title: string;
  hint: string;
}

type HintAlignment = "center" | "left" | "right";

function FieldLabel({ title, hint }: FieldLabelProps) {
  const hintBadgeRef = useRef<HTMLSpanElement | null>(null);
  const [hintAlignment, setHintAlignment] = useState<HintAlignment>("center");

  const updateHintAlignment = useCallback(() => {
    if (typeof window === "undefined") {
      return;
    }

    const badge = hintBadgeRef.current;
    if (!badge) {
      return;
    }

    const rect = badge.getBoundingClientRect();
    const viewportPadding = 16;
    const maxTooltipWidth = Math.min(320, window.innerWidth - viewportPadding * 2);
    const halfTooltipWidth = maxTooltipWidth / 2;
    const wouldOverflowLeft = rect.left - halfTooltipWidth < viewportPadding;
    const wouldOverflowRight = rect.right + halfTooltipWidth > window.innerWidth - viewportPadding;

    if (wouldOverflowRight && !wouldOverflowLeft) {
      setHintAlignment("right");
      return;
    }

    if (wouldOverflowLeft && !wouldOverflowRight) {
      setHintAlignment("left");
      return;
    }

    if (wouldOverflowLeft && wouldOverflowRight) {
      setHintAlignment("left");
      return;
    }

    setHintAlignment("center");
  }, []);

  return (
    <span className={styles.labelRow}>
      <span>{title}</span>
      <span
        ref={hintBadgeRef}
        className={styles.hintBadge}
        data-hint={hint}
        data-hint-align={hintAlignment}
        aria-label={hint}
        tabIndex={0}
        onMouseEnter={updateHintAlignment}
        onFocus={updateHintAlignment}
      >
        ?
      </span>
    </span>
  );
}

interface AdvancedSettingsModalProps {
  open: boolean;
  isRecording: boolean;
  fps: number;
  format: OutputFormat;
  codec: CodecChoice;
  preset: EncoderPreset;
  qualityMode: RecordingQualityMode;
  resolutionChoice: "fullHd" | "hd" | "sd" | "p1440" | "p2160" | "custom";
  supports4kOutput: boolean;
  crf: number;
  customWidth: number;
  customHeight: number;
  outputDir: string;
  outputName: string;
  shortcuts: RecorderShortcuts;
  onClose: () => void;
  onFpsChange: (fps: number) => void;
  onFormatChange: (format: OutputFormat) => void;
  onCodecChange: (codec: CodecChoice) => void;
  onPresetChange: (preset: EncoderPreset) => void;
  onQualityModeChange: (mode: RecordingQualityMode) => void;
  onResolutionChoiceChange: (choice: "fullHd" | "hd" | "sd" | "p1440" | "p2160" | "custom") => void;
  onCrfChange: (crf: number) => void;
  onCustomWidthInput: (value: string) => void;
  onCustomHeightInput: (value: string) => void;
  onOutputDirChange: (value: string) => void;
  onOutputNameChange: (value: string) => void;
  onPickOutputDir: () => void;
  onShortcutChange: (action: ShortcutAction, shortcut: string) => void;
}

interface ShortcutBindingCardProps {
  action: ShortcutAction;
  value: string;
  disabled: boolean;
  isCapturing: boolean;
  onCaptureStart: (action: ShortcutAction) => void;
  onCaptureCancel: () => void;
  onShortcutChange: (action: ShortcutAction, shortcut: string) => void;
}

const SHORTCUT_CARD_TITLES: Record<ShortcutAction, string> = {
  start: "Iniciar grabación",
  pauseResume: "Pausar grabación",
  stop: "Detener grabación",
};

function splitShortcutTokens(shortcut: string): string[] {
  return shortcut
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean);
}

function ShortcutBindingCard({
    action,
    value,
    disabled,
    isCapturing,
    onCaptureStart,
    onCaptureCancel,
    onShortcutChange,
}: ShortcutBindingCardProps) {
  const tokens = splitShortcutTokens(value);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (disabled) {
      return;
    }

    event.stopPropagation();
    if (!isCapturing) {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        onCaptureStart(action);
      }
      return;
    }

    event.preventDefault();
    if (event.key === "Escape") {
      onCaptureCancel();
      return;
    }

    const shortcut = buildShortcutFromKeyboardEvent(event);
    if (shortcut) {
      onShortcutChange(action, shortcut);
      onCaptureCancel();
    }
  };

  const handleClick = () => {
    if (disabled) {
      return;
    }
    onCaptureStart(action);
  };

  return (
    <div className={styles.shortcutCard}>
      <span className={styles.shortcutCardTitle}>{SHORTCUT_CARD_TITLES[action]}</span>

      <button
        type="button"
        data-shortcut-input="true"
        disabled={disabled}
        onClick={handleClick}
        onKeyDown={handleKeyDown}
        onBlur={() => {
          if (isCapturing) {
            onCaptureCancel();
          }
        }}
        aria-label={`Atajo para ${SHORTCUT_LABELS[action]}`}
        title={isCapturing ? "Presiona la nueva combinación." : "Haz clic para cambiar el atajo."}
        className={`${styles.shortcutCaptureButton} ${isCapturing ? styles.shortcutCaptureButtonActive : ""}`}
      >
        {isCapturing ? (
          <span className={styles.shortcutCaptureHint}>Presiona una tecla...</span>
        ) : (
          <span className={styles.shortcutTokenList}>
            {(tokens.length ? tokens : ["--"]).map((token, index) => (
              <span key={`${action}-${token}-${index}`} className={styles.shortcutToken}>
                {token}
              </span>
            ))}
          </span>
        )}
      </button>
    </div>
  );
}

export function AdvancedSettingsModal({
  open,
  isRecording,
  fps,
  format,
  codec,
  preset,
  qualityMode,
  resolutionChoice,
  supports4kOutput,
  crf,
  customWidth,
  customHeight,
  outputDir,
  outputName,
  shortcuts,
  onClose,
  onFpsChange,
  onFormatChange,
  onCodecChange,
  onPresetChange,
  onQualityModeChange,
  onResolutionChoiceChange,
  onCrfChange,
  onCustomWidthInput,
  onCustomHeightInput,
  onOutputDirChange,
  onOutputNameChange,
  onPickOutputDir,
  onShortcutChange,
}: AdvancedSettingsModalProps) {
  const [capturingShortcut, setCapturingShortcut] = useState<ShortcutAction | null>(null);

  useEffect(() => {
    if (isRecording) {
      setCapturingShortcut(null);
    }
  }, [isRecording]);

  const handleShortcutCaptureStart = (action: ShortcutAction) => {
    if (isRecording) {
      return;
    }
    setCapturingShortcut(action);
  };

  const handleShortcutCaptureCancel = () => {
    setCapturingShortcut(null);
  };

  const handleShortcutCaptureSave = (action: ShortcutAction, shortcut: string) => {
    onShortcutChange(action, shortcut);
    setCapturingShortcut(null);
  };

  if (!open) {
    return null;
  }

  return (
    <div className={styles.modalBackdrop} onClick={onClose}>
      <div className={styles.advancedModal} onClick={(event) => event.stopPropagation()}>
        <div className={styles.advancedHeader}>
          <h3>Configuración avanzada</h3>
          <button className={styles.closeButton} onClick={onClose} type="button" aria-label="Cerrar">
            <svg viewBox="0 0 24 24" role="img">
              <path
                d="M6.5 6.5 17.5 17.5M17.5 6.5 6.5 17.5"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>

        <div className={styles.advancedGrid}>
          <section className={styles.advancedColumn}>
            <h4>Video</h4>

            <label className={styles.field}>
              <FieldLabel
                title="FPS"
                hint="Define la fluidez del video. 30 FPS es lo recomendado para grabar juegos con buen rendimiento. 60 FPS se ve mas fluido, pero consume mas recursos."
              />
              <select value={fps} onChange={(event) => onFpsChange(Number(event.target.value))} disabled={isRecording}>
                <option value={24}>24</option>
                <option value={30}>30</option>
                <option value={60}>60</option>
                <option value={120}>120</option>
              </select>
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="Formato"
                hint="Es el tipo de archivo final. MP4 es el mas compatible para compartir y reproducir en casi cualquier dispositivo."
              />
              <select
                value={format}
                onChange={(event) => onFormatChange(event.target.value as OutputFormat)}
                disabled={isRecording}
              >
                <option value="mp4">mp4</option>
                <option value="mkv">mkv</option>
                <option value="webM">webm</option>
              </select>
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="Codec"
                hint="Compresion de video. En Auto la app elige el mejor encoder disponible en tu equipo (NVENC, AMF, QSV o CPU) para mantener mejor rendimiento."
              />
              <select
                value={codec}
                onChange={(event) => onCodecChange(event.target.value as CodecChoice)}
                disabled={format === "webM" || isRecording}
              >
                <option value="auto">Auto (recomendado)</option>
                <option value="nvenc">NVENC (NVIDIA)</option>
                <option value="amf">AMF (AMD)</option>
                <option value="qsv">QSV (Intel)</option>
                <option value="h264">H.264</option>
                <option value="h265">H.265</option>
                <option value="vp9">VP9</option>
              </select>
            </label>
          </section>

          <section className={styles.advancedColumn}>
            <h4>Codificación</h4>

            <label className={styles.field}>
              <FieldLabel
                title="Calidad"
                hint="Controla el balance entre calidad y rendimiento. Calidad usa mejor escalado y evita downscale temprano."
              />
              <select
                value={qualityMode}
                onChange={(event) => onQualityModeChange(event.target.value as RecordingQualityMode)}
                disabled={isRecording}
              >
                <option value="performance">Rendimiento</option>
                <option value="balanced">Balanceado</option>
                <option value="quality">Calidad</option>
              </select>
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="Preset"
                hint="Velocidad de codificacion. ultra fast reduce el impacto en FPS del juego. fast/medium comprimen mejor, pero exigen mas CPU/GPU."
              />
              <select
                value={preset}
                onChange={(event) => onPresetChange(event.target.value as EncoderPreset)}
                disabled={isRecording}
              >
                <option value="ultraFast">ultra fast</option>
                <option value="fast">fast</option>
                <option value="medium">medium</option>
              </select>
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="CRF"
                hint="Calidad visual. Numero mas bajo = mejor calidad y archivo mas grande. Para H.264, un rango entre 20 y 24 suele dar buen resultado."
              />
              <div className={styles.crfRow}>
                <input
                  type="range"
                  min={0}
                  max={51}
                  value={crf}
                  onChange={(event) => onCrfChange(Number(event.target.value))}
                  disabled={isRecording}
                />
                <span>{crf}</span>
              </div>
            </label>

          </section>

          <section className={styles.advancedColumn}>
            <h4>Salida</h4>

            <label className={styles.field}>
              <FieldLabel
                title="Carpeta"
                hint="Lugar donde se guarda la grabacion al finalizar."
              />
              <div className={styles.pathField}>
                <input
                  value={outputDir}
                  onChange={(event) => onOutputDirChange(event.target.value)}
                  placeholder="~/Videos/Recordings"
                  disabled={isRecording}
                />
                <button
                  className={styles.iconButton}
                  onClick={onPickOutputDir}
                  type="button"
                  disabled={isRecording}
                  aria-label="Elegir carpeta"
                >
                  <svg viewBox="0 0 24 24" role="img">
                    <path
                      d="M4 7.2a2 2 0 0 1 2-2h4.4l2 2H18a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7.2Z"
                      fill="currentColor"
                    />
                  </svg>
                </button>
              </div>
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="Nombre del archivo"
                hint="Nombre base del video. Si no incluyes {date} y {time}, la app los agrega automaticamente para evitar archivos con el mismo nombre."
              />
              <input
                value={outputName}
                onChange={(event) => onOutputNameChange(event.target.value)}
                placeholder="Video"
                disabled={isRecording}
              />
            </label>

            <label className={styles.field}>
              <FieldLabel
                title="Resolucion"
                hint="Tamano final del video. Bajar resolucion (por ejemplo 1280x720) ayuda a evitar caidas de FPS durante la grabacion."
              />
              <select
                value={resolutionChoice}
                onChange={(event) =>
                  onResolutionChoiceChange(
                    event.target.value as "fullHd" | "hd" | "sd" | "p1440" | "p2160" | "custom",
                  )
                }
                disabled={isRecording}
              >
                {supports4kOutput && <option value="p2160">4K (3840x2160)</option>}
                <option value="p1440">1440p (2560x1440)</option>
                <option value="fullHd">1080p (1920x1080)</option>
                <option value="hd">720p (1280x720)</option>
                <option value="sd">480p (854x480)</option>
                <option value="custom">Personalizada</option>
              </select>
              {resolutionChoice === "custom" && (
                <div className={styles.resolutionRow}>
                  <input
                    type="number"
                    min={1}
                    value={customWidth}
                    onChange={(event) => onCustomWidthInput(event.target.value)}
                    disabled={isRecording}
                  />
                  <span className={styles.resolutionSep}>x</span>
                  <input
                    type="number"
                    min={1}
                    value={customHeight}
                    onChange={(event) => onCustomHeightInput(event.target.value)}
                    disabled={isRecording}
                  />
                </div>
              )}
            </label>
          </section>

          <section className={styles.shortcutSection}>
            <h4>Atajos de teclado</h4>
            <div className={styles.shortcutGrid}>
              <ShortcutBindingCard
                action="start"
                value={shortcuts.start}
                disabled={isRecording}
                isCapturing={capturingShortcut === "start"}
                onCaptureStart={handleShortcutCaptureStart}
                onCaptureCancel={handleShortcutCaptureCancel}
                onShortcutChange={handleShortcutCaptureSave}
              />
              <ShortcutBindingCard
                action="pauseResume"
                value={shortcuts.pauseResume}
                disabled={isRecording}
                isCapturing={capturingShortcut === "pauseResume"}
                onCaptureStart={handleShortcutCaptureStart}
                onCaptureCancel={handleShortcutCaptureCancel}
                onShortcutChange={handleShortcutCaptureSave}
              />
              <ShortcutBindingCard
                action="stop"
                value={shortcuts.stop}
                disabled={isRecording}
                isCapturing={capturingShortcut === "stop"}
                onCaptureStart={handleShortcutCaptureStart}
                onCaptureCancel={handleShortcutCaptureCancel}
                onShortcutChange={handleShortcutCaptureSave}
              />
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
