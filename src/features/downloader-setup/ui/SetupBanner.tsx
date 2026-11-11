import { bannerState, bannerText } from "../model/toolsStatus.ts";
import { useToolsStore } from "../model/toolsStore.ts";
import { useToolsStatus } from "./useToolsStatus.ts";
import styles from "./downloader-setup.module.css";

/** A slim, non-blocking bar while yt-dlp, deno and ffmpeg are installed the first time, or when that failed. */
export const SetupBanner = () => {
  const status = useToolsStatus();
  const check = useToolsStore((s) => s.check);
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
