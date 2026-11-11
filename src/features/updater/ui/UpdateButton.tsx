import { useEffect } from "react";
import { downloadPercent, useUpdateStore } from "../model/updateStore";
import { useModalStore } from "@/shared/store/modalStore";
import { UpdateDialog } from "./UpdateDialog";
import styles from "./updater.module.css";

/** Delay before the startup check, so it does not compete with loading the library. */
const STARTUP_CHECK_DELAY_MS = 5000;
/** The app can stay open for days: look again every hour. */
export const PERIODIC_CHECK_MS = 60 * 60 * 1000;

/** Header pill that appears only when a newer version is out. Also runs the silent startup and periodic checks. */
export const UpdateButton = () => {
  const { status, version, downloaded, total, checkForUpdate } = useUpdateStore();
  const openModal = useModalStore((s) => s.openModal);

  useEffect(() => {
    if (import.meta.env.DEV) return;
    const timer = setTimeout(
      () => checkForUpdate({ silent: true }),
      STARTUP_CHECK_DELAY_MS,
    );
    const interval = setInterval(() => checkForUpdate({ silent: true }), PERIODIC_CHECK_MS);
    return () => {
      clearTimeout(timer);
      clearInterval(interval);
    };
  }, [checkForUpdate]);

  if (status !== "available" && status !== "downloading" && status !== "installing") {
    return null;
  }

  const percent = downloadPercent(downloaded, total);
  const label =
    status === "available"
      ? `Update to v${version}`
      : status === "installing"
        ? "Installing…"
        : `Updating${percent === null ? "…" : ` ${percent}%`}`;

  return (
    <button className={styles.pill} onClick={() => openModal(<UpdateDialog />)}>
      {label}
    </button>
  );
};
