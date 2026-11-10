import { installLabel, useUpdateStore } from "../model/updateStore";
import { useModalStore } from "@/shared/store/modalStore";
import styles from "./updater.module.css";

export const UpdateDialog = () => {
  const { status, version, notes, downloaded, total, error, installUpdate } =
    useUpdateStore();
  const closeModal = useModalStore((s) => s.closeModal);
  const busy = status === "downloading" || status === "installing";

  return (
    <div className={styles.dialog}>
      <h2 className={styles.dialogTitle}>Update available: v{version}</h2>
      <p className={styles.muted}>
        The app restarts to finish the update. Playback stops.
      </p>
      {notes && <div className={styles.notes}>{notes}</div>}
      {status === "error" && error && <p className={styles.error}>{error}</p>}
      <div className={styles.actions}>
        <button className={styles.secondary} onClick={closeModal} disabled={busy}>
          Later
        </button>
        <button className={styles.primary} onClick={installUpdate} disabled={busy}>
          {installLabel(status, version, downloaded, total)}
        </button>
      </div>
    </div>
  );
};
