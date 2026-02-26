import styles from "./Alerts.module.css";

interface AlertsProps {
  lastError: string | null;
  errorMsg: string;
  isProcessing?: boolean;
}

export function Alerts({ lastError, errorMsg, isProcessing = false }: AlertsProps) {
  if (!lastError && !errorMsg && !isProcessing) {
    return null;
  }

  return (
    <section className={styles.alerts}>
      {isProcessing && (
        <div className={styles.infoInline}>
          Procesando el video final. Espera mientras se mezcla el audio.
        </div>
      )}
      {lastError && <div className={styles.warningInline}>Error del backend: {lastError}</div>}
      {errorMsg && <div className={styles.warningInline}>{errorMsg}</div>}
    </section>
  );
}
