import gearSolidFull from "../../assets/gear-solid-full.svg";
import { openUrl } from "@tauri-apps/plugin-opener";
import styles from "./TopBar.module.css";

interface TopBarProps {
  onOpenSettings: () => void;
}

export function TopBar({ onOpenSettings }: TopBarProps) {
  const authorUrl = "https://github.com/xmunder";

  const openAuthorProfile = async () => {
    try {
      await openUrl(authorUrl);
    } catch {
      window.open(authorUrl, "_blank", "noopener,noreferrer");
    }
  };

  return (
    <header className={styles.topbar}>
      <div className={styles.brand}>
        <span className={styles.brandDot} aria-hidden />
        <span>Capturist</span>
      </div>
      <div className={styles.rightArea}>
        <a
          className={styles.authorLink}
          href={authorUrl}
          target="_blank"
          rel="noreferrer"
          onClick={(event) => {
            event.preventDefault();
            void openAuthorProfile();
          }}
        >
          xMunder (Github)
        </a>
        <button
          className={styles.iconButton}
          type="button"
          onClick={onOpenSettings}
          aria-label="Configuración avanzada"
        >
          <img src={gearSolidFull} alt="" aria-hidden />
        </button>
      </div>
    </header>
  );
}
