import { useEffect, useState } from "react";
import { DefaultQualitySelect } from "@/features/download-quality/ui/DefaultQualitySelect.tsx";
import {
  getDefaultDownloadQuality,
  setDefaultDownloadQuality,
  type DownloadQualityPreset,
} from "@/shared/lib/audio.ts";
import {
  anyBusy,
  anyFailed,
  TOOL_LABELS,
  TOOL_NAMES,
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
  selectClassName?: string;
}

/** Settings rows for Default Download Quality and yt-dlp, deno and ffmpeg setup/updates. */
export const DownloaderSettings = ({
  rowClassName,
  buttonClassName,
  mutedClassName,
  selectClassName,
}: Props) => {
  const status = useToolsStatus();
  const check = useToolsStore((s) => s.check);
  const busy = anyBusy(status);
  const [defaultQuality, setDefaultQuality] = useState<DownloadQualityPreset>("best");

  useEffect(() => {
    getDefaultDownloadQuality()
      .then(setDefaultQuality)
      .catch((err) => console.error("Failed to load default download quality:", err));
  }, []);

  const handleQualityChange = async (next: DownloadQualityPreset) => {
    const prev = defaultQuality;
    setDefaultQuality(next);
    try {
      await setDefaultDownloadQuality(next);
    } catch (err) {
      console.error("Failed to save default download quality:", err);
      setDefaultQuality(prev);
    }
  };

  return (
    <>
      <div className={rowClassName}>
        <label htmlFor="default-download-quality-select" className={styles.toolName}>
          <span>Default Download Quality</span>
          <span className={mutedClassName}>
            Used by Quick Download across available sources.
          </span>
        </label>
        <DefaultQualitySelect id="default-download-quality-select" className={selectClassName}
          value={defaultQuality} onChange={(quality) => void handleQualityChange(quality)} />
      </div>
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
            </span>
            <span
              className={tool?.phase === "failed" ? styles.toolFailed : `${mutedClassName} ${styles.toolStatus}`}
              title={tool?.error ?? undefined}
            >
              {tool ? toolStatusText(tool) : "Loading…"}
            </span>
          </div>
        );
      })}
      <div className={rowClassName}>
        <span className={mutedClassName}>
          Tools update automatically in the background.
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
