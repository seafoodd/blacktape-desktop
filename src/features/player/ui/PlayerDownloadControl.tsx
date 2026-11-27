import { CancelDownloadButton } from "@/shared/ui/cancel-download-button/CancelDownloadButton.tsx";
import styles from "@/features/player/ui/player-controls.module.css";

export const PlayerDownloadControl = ({ taskId, percent }: { taskId: string; percent: number }) => (
  <div className={styles.downloadProgress}>
    <span className={styles.downloadStatus} role="status">Downloading {percent}%</span>
    <span className={styles.downloadSpinner} aria-hidden="true" />
    <CancelDownloadButton taskId={taskId} />
  </div>
);
