import { useModalStore } from "@/shared/store/modalStore.ts";
import styles from "./cancel-download-modal.module.css";

interface CancelDownloadModalProps {
  title: string;
  artist?: string;
  onConfirm: () => void;
}

export const CancelDownloadModal = ({
  title,
  artist,
  onConfirm,
}: CancelDownloadModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Cancel Download</h2>
      <p className={styles.text}>
        Are you sure you want to cancel downloading{" "}
        <span className={styles.itemHighlight}>"{title}"</span>
        {artist ? ` by ${artist}` : ""}?
      </p>
      <div className={styles.buttons}>
        <button className={styles.keepButton} onClick={closeModal}>
          Keep Downloading
        </button>
        <button
          className={styles.cancelDownloadButton}
          onClick={() => {
            onConfirm();
            closeModal();
          }}
        >
          Cancel Download
        </button>
      </div>
    </div>
  );
};

export default CancelDownloadModal;
