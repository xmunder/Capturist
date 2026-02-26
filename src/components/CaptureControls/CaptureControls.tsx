import type { CaptureState, CaptureTarget } from "../../recorder/types";
import displaySolidFull from "../../assets/display-solid-full.svg";
import cropSimpleSolidFull from "../../assets/crop-simple-solid-full.svg";
import styles from "./CaptureControls.module.css";

interface CaptureControlsProps {
  targets: CaptureTarget[];
  selectedTargetId: number | null;
  supported: boolean | null;
  status: CaptureState;
  busy: boolean;
  isProcessing: boolean;
  isRecording: boolean;
  showCropControls: boolean;
  cropEnabled: boolean;
  captureSystemAudio: boolean;
  captureMicrophoneAudio: boolean;
  microphoneGainPercent: number;
  audioInputDevices: string[];
  selectedMicrophoneDevice: string | null;
  onSelectTarget: (targetId: number | null) => void;
  onRefreshTargets: () => void;
  onToggleCropEnabled: (enabled: boolean) => void;
  onOpenRegionOverlay: () => void;
  onCaptureSystemAudioChange: (enabled: boolean) => void | Promise<void>;
  onCaptureMicrophoneAudioChange: (enabled: boolean) => void | Promise<void>;
  onMicrophoneGainPercentChange: (value: number) => void;
  onSelectMicrophoneDevice: (device: string | null) => void;
  onRefreshMicrophoneDevices: () => void;
  onStart: () => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}

export function CaptureControls({
  targets,
  selectedTargetId,
  supported,
  status,
  busy,
  isProcessing,
  isRecording,
  showCropControls,
  cropEnabled,
  captureSystemAudio,
  captureMicrophoneAudio,
  microphoneGainPercent,
  audioInputDevices,
  selectedMicrophoneDevice,
  onSelectTarget,
  onRefreshTargets,
  onToggleCropEnabled,
  onOpenRegionOverlay,
  onCaptureSystemAudioChange,
  onCaptureMicrophoneAudioChange,
  onMicrophoneGainPercentChange,
  onSelectMicrophoneDevice,
  onRefreshMicrophoneDevices,
  onStart,
  onPause,
  onResume,
  onStop,
}: CaptureControlsProps) {
  const canStart = !busy && !isProcessing && status === "idle" && Boolean(supported);
  const canPauseOrResume = !busy && (status === "running" || status === "paused");
  const canStop = !busy && (status === "running" || status === "paused");
  const canToggleAudio = !busy && status !== "stopped";
  const canAdjustMicrophoneGain = !busy && !isRecording;
  const canSelectMicrophoneDevice = !busy && !isRecording && audioInputDevices.length > 0;
  return (
    <div className={styles.captureColumn}>
      <div className={styles.captureSpacer} />

      <div className={styles.captureControls}>
        <label className={styles.sectionTitle}>Fuente de captura</label>

        <div className={styles.targetRow}>
          <div className={styles.captureSelectWrap}>
            <select
              className={styles.captureSelect}
              value={selectedTargetId ?? ""}
              onChange={(event) => {
                const value = event.target.value;
                onSelectTarget(value ? Number(value) : null);
              }}
              disabled={isRecording}
            >
              {targets.length === 0 && <option value="">Sin targets detectados</option>}
              {targets.map((target) => (
                <option key={target.id} value={target.id}>
                  {target.name}
                </option>
              ))}
            </select>
            <img src={displaySolidFull} alt="" aria-hidden className={styles.selectIcon} />
          </div>

          <button
            className={styles.iconButton}
            type="button"
            onClick={onRefreshTargets}
            disabled={busy || isRecording}
            aria-label="Actualizar fuentes"
          >
            <svg viewBox="0 0 24 24" role="img">
              <path
                d="M19 6.5V10h-3.5M5 17.5V14h3.5M18 10.2a7 7 0 0 0-11.7-2M6 13.8a7 7 0 0 0 11.7 2"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        </div>

        {showCropControls && (
          <>
            <label className={styles.sectionTitle}>Área de captura</label>
            <div className={styles.captureAreaRow}>
              <button
                className={`${styles.captureAreaButton} ${!cropEnabled ? styles.captureAreaButtonActive : ""}`}
                type="button"
                onClick={() => onToggleCropEnabled(false)}
                disabled={isRecording}
              >
                <img src={displaySolidFull} alt="" aria-hidden />
                Pantalla completa
              </button>
              <button
                className={`${styles.captureAreaButton} ${cropEnabled ? styles.captureAreaButtonActive : ""}`}
                type="button"
                onClick={() => void onOpenRegionOverlay()}
                disabled={isRecording}
              >
                <img src={cropSimpleSolidFull} alt="" aria-hidden />
                Región
              </button>
            </div>
          </>
        )}

        <label className={styles.sectionTitle}>Audio</label>
        <div className={styles.audioCard}>
          <div className={styles.audioRow}>
            <span className={styles.audioLabel}>
              <svg viewBox="0 0 24 24" className={styles.audioToggleIcon} aria-hidden="true">
                <path
                  d="M4.5 10.2v3.6h2.7l3.4 3V7.2l-3.4 3H4.5Z"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinejoin="round"
                />
                <path
                  d="M14.2 9.2a4.1 4.1 0 0 1 0 5.6M16.7 6.8a7.3 7.3 0 0 1 0 10.4"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                />
              </svg>
              Audio del sistema
            </span>
            <button
              type="button"
              className={`${styles.switchToggle} ${captureSystemAudio ? styles.switchToggleActive : ""}`}
              onClick={() => void onCaptureSystemAudioChange(!captureSystemAudio)}
              disabled={!canToggleAudio}
              aria-label="Alternar audio del sistema"
            >
              <span className={styles.switchThumb} />
            </button>
          </div>

          <div className={styles.audioRow}>
            <span className={styles.audioLabel}>
              <svg viewBox="0 0 24 24" className={styles.audioToggleIcon} aria-hidden="true">
                <path
                  d="M12 15.2a3.2 3.2 0 0 0 3.2-3.2V7.8a3.2 3.2 0 1 0-6.4 0V12a3.2 3.2 0 0 0 3.2 3.2Z"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                />
                <path
                  d="M6.5 12.2a5.5 5.5 0 1 0 11 0M12 17.8V20.2M9.4 20.2h5.2"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.8"
                  strokeLinecap="round"
                />
              </svg>
              Micrófono
            </span>
            <button
              type="button"
              className={`${styles.switchToggle} ${captureMicrophoneAudio ? styles.switchToggleActive : ""}`}
              onClick={() => void onCaptureMicrophoneAudioChange(!captureMicrophoneAudio)}
              disabled={!canToggleAudio}
              aria-label="Alternar micrófono"
            >
              <span className={styles.switchThumb} />
            </button>
          </div>

          {captureMicrophoneAudio && (
            <div className={styles.microphoneSettings}>
              <div className={styles.deviceRow}>
                <span className={styles.gainLabel}>Dispositivo</span>
                <div className={styles.audioDeviceField}>
                  <select
                    className={styles.audioDeviceSelect}
                    value={selectedMicrophoneDevice ?? ""}
                    onChange={(event) =>
                      onSelectMicrophoneDevice(event.target.value ? event.target.value : null)
                    }
                    disabled={!canSelectMicrophoneDevice}
                  >
                    {audioInputDevices.length === 0 ? (
                      <option value="">No se detectaron dispositivos</option>
                    ) : (
                      audioInputDevices.map((device) => (
                        <option key={device} value={device}>
                          {device}
                        </option>
                      ))
                    )}
                  </select>
                  <button
                    className={styles.iconButton}
                    type="button"
                    onClick={onRefreshMicrophoneDevices}
                    disabled={isRecording || busy}
                    aria-label="Actualizar dispositivos de micrófono"
                  >
                    <svg viewBox="0 0 24 24" role="img">
                      <path
                        d="M20 12a8 8 0 1 1-2.34-5.66"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                      />
                      <path
                        d="M20 4v6h-6"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </button>
                </div>
              </div>

              <div className={styles.gainRow}>
                <span className={styles.gainLabel}>Ganancia</span>
                <input
                  type="range"
                  min={0}
                  max={400}
                  value={microphoneGainPercent}
                  onChange={(event) => onMicrophoneGainPercentChange(Number(event.target.value))}
                  disabled={!canAdjustMicrophoneGain}
                  className={styles.gainSlider}
                />
                <span className={styles.gainValue}>{microphoneGainPercent}%</span>
              </div>
            </div>
          )}
        </div>

        {!supported && supported !== null && (
          <div className={styles.warningInline}>
            La captura no está soportada o no hay permisos.
          </div>
        )}

        <div className={styles.actions}>
          <button className={`${styles.actionButton} ${styles.actionPrimary}`} onClick={onStart} disabled={!canStart}>
            <svg viewBox="0 0 24 24" role="img">
              <path d="M8 6.8 18 12 8 17.2V6.8Z" fill="currentColor" />
            </svg>
            Iniciar
          </button>

          <button
            className={styles.actionButton}
            onClick={status === "paused" ? onResume : onPause}
            disabled={!canPauseOrResume}
          >
            <svg viewBox="0 0 24 24" role="img">
              {status === "paused" ? (
                <path d="M8 6.8 18 12 8 17.2V6.8Z" fill="currentColor" />
              ) : (
                <path d="M8.5 7h2.8v10H8.5zM12.7 7h2.8v10h-2.8z" fill="currentColor" />
              )}
            </svg>
            {status === "paused" ? "Reanudar" : "Pausar"}
          </button>

          <button className={`${styles.actionButton} ${styles.actionDanger}`} onClick={onStop} disabled={!canStop}>
            <svg viewBox="0 0 24 24" role="img">
              <rect x="8" y="8" width="8" height="8" rx="1.2" fill="currentColor" />
            </svg>
            Detener
          </button>
        </div>

        <div className={styles.secondaryActions} />
      </div>
    </div>
  );
}
