import clsx from "clsx";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { isRunning } from "@/shared/lib/library/downloadMatch.ts";
import { CancelDownloadButton } from "@/shared/ui/cancel-download-button/CancelDownloadButton.tsx";
import styles from "@/shared/ui/track-download-progress/track-download-progress.module.css";

export const TrackDownloadProgress = ({ taskId, percent, finished = false }: {
  taskId?: string;
  percent: number;
  finished?: boolean;
}) => {
  const running = useDownloadStore((state) => isRunning(taskId ? state.activeDownloads[taskId] : undefined));
  const cancellable = running && !finished;
  return (
    <span className={clsx(styles.slot, cancellable && styles.cancellable)}>
      <span className={clsx(styles.percentage, percent >= 100 ? styles.completed : percent > 0 ? styles.active : styles.pending)} aria-live="polite">
        {percent}%
      </span>
      {cancellable && taskId && <CancelDownloadButton taskId={taskId} />}
    </span>
  );
};
