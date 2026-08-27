import { getCurrentWindow } from "@tauri-apps/api/window";
import styles from "./window-controls.module.css";
import MinimizeIcon from "@/assets/icons/minimize.svg?react";
import MaximizeIcon from "@/assets/icons/maximize.svg?react";
import CloseIcon from "@/assets/icons/close.svg?react";

const appWindow = getCurrentWindow();

export const WindowControls = () => {
  return (
    <div className={styles.controls}>
      <button
        onClick={() => appWindow.minimize()}
        className={styles.button}
        aria-label="Minimize"
      >
        <MinimizeIcon width={12} height={12} />
      </button>
      <button
        onClick={() => appWindow.toggleMaximize()}
        className={styles.button}
        aria-label="Maximize"
      >
        <MaximizeIcon width={12} height={12} />
      </button>
      <button
        onClick={() => appWindow.close()}
        className={`${styles.button} ${styles.close}`}
        aria-label="Close"
      >
        <CloseIcon width={12} height={12} />
      </button>
    </div>
  );
};
