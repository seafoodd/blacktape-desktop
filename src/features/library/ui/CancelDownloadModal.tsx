import { useState } from "react";
import { useModalStore } from "@/shared/store/modalStore.ts";
import styles from "./cancel-download-modal.module.css";

interface CancelDownloadModalProps {
  title: string;
  artist?: string;
  taskCount?: number;
  onConfirm: () => void | Promise<void>;
}

export const CancelDownloadModal = ({
  title,
  artist,
  taskCount = 1,
  onConfirm,
}: CancelDownloadModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Cancel Download</h2>
      <p className={styles.text}>
        Are you sure you want to cancel downloading{" "}
        <span className={styles.itemHighlight}>"{title}"</span>
        {artist ? ` by ${artist}` : ""}?
      </p>
      <p className={styles.text}>
        {taskCount > 1 ? `This stops all ${taskCount} selected download tasks.` : "This stops the entire download task."}
        {" "}Songs already saved stay in your library.
      </p>
      {error && <p className={styles.error} role="alert">Could not cancel the download. {error}</p>}
      <div className={styles.buttons}>
        <button className={styles.keepButton} onClick={closeModal} disabled={pending}>
          Keep Downloading
        </button>
        <button
          className={styles.cancelDownloadButton}
          disabled={pending}
          onClick={async () => {
            setPending(true);
            setError(null);
            try {
              await onConfirm();
              closeModal();
            } catch (error) {
              setError(String(error));
              setPending(false);
            }
          }}
        >
          {pending ? "Cancelling…" : "Cancel Download"}
        </button>
      </div>
    </div>
  );
};

export default CancelDownloadModal;
