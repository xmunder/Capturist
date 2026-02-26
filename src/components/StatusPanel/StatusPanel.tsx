import type { CaptureState } from "../../recorder/types";
import styles from "./StatusPanel.module.css";

interface StatusPanelProps {
  status: CaptureState;
  statusLabel: string;
  elapsedLabel: string;
  resolutionLabel: string;
  codecLabel: string;
  captureModeLabel: string;
  systemCaptureLabel: string;
  microphoneCaptureLabel: string;
  outputDirLabel: string;
}

const DOT_COLOR_BY_STATE: Record<CaptureState, string> = {
  idle: "#a4a9b1",
  running: "#4cd188",
  paused: "#f4bd3b",
  stopped: "#a4a9b1",
};

export function StatusPanel({
  status,
  statusLabel,
  elapsedLabel,
  resolutionLabel,
  codecLabel,
  captureModeLabel,
  systemCaptureLabel,
  microphoneCaptureLabel,
  outputDirLabel,
}: StatusPanelProps) {
  return (
    <aside className={styles.statusColumn}>
      <div className={styles.statusChip}>
        <span
          className={styles.statusDot}
          style={{ backgroundColor: DOT_COLOR_BY_STATE[status] }}
          aria-hidden
        />
        {statusLabel}
      </div>

      <div className={styles.statusList}>
        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
              <circle
                cx="12"
                cy="12"
                r="7.2"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
              />
              <path
                d="M12 8.3v3.9l2.6 1.5"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
              />
            </svg>
            Tiempo
          </span>
          <strong className={`${styles.statusValue} ${styles.elapsedValue}`}>
            {elapsedLabel}
          </strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
              <rect
                x="4.5"
                y="5"
                width="15"
                height="11"
                rx="1.8"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
              />
              <path
                d="M9 19h6"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
              />
            </svg>
            Resolución
          </span>
          <strong className={styles.statusValue}>{resolutionLabel}</strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
              <rect
                x="6"
                y="4.5"
                width="12"
                height="15"
                rx="1.6"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
              />
              <path
                d="M9.4 8h5.2M9.4 11.3h5.2M9.4 14.6h3.6"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
              />
            </svg>
            Codec
          </span>
          <strong className={styles.statusValue}>{codecLabel}</strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
              <path
                d="M3.7 8.5a2 2 0 0 1 2-2h4.6l2 2h6a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H5.7a2 2 0 0 1-2-2v-8Z"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinejoin="round"
              />
            </svg>
            Guardado
          </span>
          <strong className={styles.statusValue}>{outputDirLabel}</strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
              <path
                d="M6.8 7.2h2.6v2.6H6.8V7.2Zm7.8 0h2.6v2.6h-2.6V7.2Zm-7.8 7h2.6v2.6H6.8v-2.6Zm7.8 0h2.6v2.6h-2.6v-2.6Z"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
              />
              <path
                d="M3.8 3.8h16.4v16.4H3.8z"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
              />
            </svg>
            Captura
          </span>
          <strong className={styles.statusValue}>{captureModeLabel}</strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
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
            Sistema
          </span>
          <strong className={styles.statusValue}>{systemCaptureLabel}</strong>
        </div>

        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>
            <svg viewBox="0 0 24 24" role="img">
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
          <strong className={styles.statusValue}>{microphoneCaptureLabel}</strong>
        </div>
      </div>
    </aside>
  );
}
