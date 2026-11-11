import {
  anyBusy,
  anyFailed,
  TOOL_LABELS,
  TOOL_NAMES,
  TOOL_PURPOSE,
  toolDetails,
  toolStatusText,
} from "../model/toolsStatus.ts";
import { useToolsStore } from "../model/toolsStore.ts";
import { useToolsStatus } from "./useToolsStatus.ts";
import styles from "./downloader-setup.module.css";

interface Props {
  rowClassName: string;
  buttonClassName: string;
  mutedClassName: string;
}

/** Settings rows for yt-dlp, deno and ffmpeg: version, where it comes from, state, and a manual check. */
export const DownloaderSettings = ({ rowClassName, buttonClassName, mutedClassName }: Props) => {
  const status = useToolsStatus();
  const check = useToolsStore((s) => s.check);
  const busy = anyBusy(status);

  return (
    <>
      {TOOL_NAMES.map((name) => {
        const tool = status?.[name];
        const details = tool ? toolDetails(tool) : "";
        return (
          <div key={name} className={rowClassName}>
            <span className={styles.toolName}>
              <span>
                {TOOL_LABELS[name]}
                {details && <span className={mutedClassName}> {details}</span>}
              </span>
              <span className={mutedClassName}>{TOOL_PURPOSE[name]}</span>
            </span>
            <span
              className={tool?.phase === "failed" ? styles.toolFailed : mutedClassName}
              title={tool?.error ?? undefined}
            >
              {tool ? toolStatusText(tool) : "Loading…"}
            </span>
          </div>
        );
      })}
      <div className={rowClassName}>
        <span className={mutedClassName}>
          yt-dlp is checked for updates daily, Deno and FFmpeg weekly. A Deno or FFmpeg you installed yourself is used as is.
        </span>
        <button
          className={buttonClassName}
          onClick={() => check().catch(console.error)}
          disabled={busy || !status}
        >
          {anyFailed(status) ? "Retry" : "Check for updates"}
        </button>
      </div>
    </>
  );
};
