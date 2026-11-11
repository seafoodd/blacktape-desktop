import { useEffect, useState } from "react";
import { downloadPercent, useUpdateStore } from "../model/updateStore";
import {
  CHECK_INTERVAL_CHANNEL,
  checkIntervalMs,
  parseCheckInterval,
  readCheckInterval,
} from "../model/checkInterval";
import { useModalStore } from "@/shared/store/modalStore";
import { UpdateDialog } from "./UpdateDialog";
import styles from "./updater.module.css";

/** Delay before the startup check, so it does not compete with loading the library. */
const STARTUP_CHECK_DELAY_MS = 5000;

/**
 * Header pill that appears only when a newer version is out. Also runs the silent startup check and the periodic
 * ones (the app can stay open for days), as often as Settings > About says.
 */
export const UpdateButton = () => {
  const { status, version, downloaded, total, checkForUpdate } = useUpdateStore();
  const openModal = useModalStore((s) => s.openModal);
  const [intervalHours, setIntervalHours] = useState(readCheckInterval);

  useEffect(() => {
    if (import.meta.env.DEV) return;
    const timer = setTimeout(
      () => checkForUpdate({ silent: true }),
      STARTUP_CHECK_DELAY_MS,
    );
    return () => clearTimeout(timer);
  }, [checkForUpdate]);

  // the choice is made in the settings window
  useEffect(() => {
    const channel = new BroadcastChannel(CHECK_INTERVAL_CHANNEL);
    channel.onmessage = (e: MessageEvent<{ type?: string; hours?: number }>) => {
      if (e.data?.type === "CHECK_INTERVAL_CHANGED") setIntervalHours(parseCheckInterval(String(e.data.hours)));
    };
    return () => channel.close();
  }, []);

  useEffect(() => {
    const ms = checkIntervalMs(intervalHours);
    if (import.meta.env.DEV || ms === null) return;
    const interval = setInterval(() => checkForUpdate({ silent: true }), ms);
    return () => clearInterval(interval);
  }, [checkForUpdate, intervalHours]);

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
