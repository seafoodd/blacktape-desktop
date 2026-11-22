import { useEffect } from "react";
import { bannerState, bannerText } from "../model/toolsStatus.ts";
import { useToolsStore } from "../model/toolsStore.ts";
import { useToolsStatus } from "./useToolsStatus.ts";
import {
  initScanProgressListener,
  scanBannerText,
  useScanStore,
} from "@/features/library/model/scanStore.ts";
import styles from "./downloader-setup.module.css";

/** A slim, non-blocking bar while scanning music or installing downloader dependencies. */
export const SetupBanner = () => {
  const scanProgress = useScanStore((s) => s.progress);
  const status = useToolsStatus();
  const check = useToolsStore((s) => s.check);

  useEffect(() => {
    const unlisten = initScanProgressListener();
    return () => unlisten();
  }, []);

  const scanText = scanBannerText(scanProgress);
  if (scanText) {
    const scanPercent =
      scanProgress.total > 0
        ? Math.min(100, (scanProgress.scanned / scanProgress.total) * 100)
        : null;

    return (
      <div className={styles.banner} role="status">
        <span className="truncate" title={scanText}>
          {scanText}
        </span>
        {scanPercent !== null && (
          <div
            className={styles.bannerProgress}
            style={{ width: `${scanPercent}%` }}
          />
        )}
      </div>
    );
  }

  const state = bannerState(status);
  if (!state) return null;

  const percent =
    state.kind === "settingUp" && state.total ? Math.min(100, (state.downloaded / state.total) * 100) : null;

  return (
    <div className={styles.banner} role="status">
      <span className="truncate" title={bannerText(state)}>
        {bannerText(state)}
      </span>
      {state.kind === "failed" && (
        <button className={styles.bannerButton} onClick={() => check().catch(console.error)}>
          Retry
        </button>
      )}
      {percent !== null && <div className={styles.bannerProgress} style={{ width: `${percent}%` }} />}
    </div>
  );
};
