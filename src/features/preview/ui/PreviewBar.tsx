import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { BiPause, BiPlay, BiX } from "react-icons/bi";
import { previewGetState, type PreviewState } from "@/shared/lib/preview.ts";
import { formatDurationSecs } from "@/pages/search-results/searchResultsFormat.ts";
import { usePreviewStore } from "../model/previewStore.ts";
import styles from "./preview.module.css";

/** Floating mini player of the streaming preview; renders nothing while idle. Mounted once in the app shell. */
export const PreviewBar = () => {
  const state = usePreviewStore((s) => s.state);
  const apply = usePreviewStore((s) => s.apply);
  const toggle = usePreviewStore((s) => s.toggle);
  const seek = usePreviewStore((s) => s.seek);
  const stop = usePreviewStore((s) => s.stop);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    previewGetState()
      .then((s) => !disposed && apply(s))
      .catch(() => undefined);
    listen<PreviewState>("preview-state", (e) => apply(e.payload)).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [apply]);

  if (state.status === "idle") return null;

  const duration = state.duration_ms ?? 0;
  const fraction = duration > 0 ? Math.min(1, state.position_ms / duration) : 0;
  const loading = state.status === "loading";
  const failed = state.status === "error";

  return (
    <div className={styles.bar} role="region" aria-label="Preview">
      {state.cover ? (
        <img className={styles.barCover} src={state.cover} alt="" referrerPolicy="no-referrer" />
      ) : (
        <div className={styles.barCover} aria-hidden="true" />
      )}
      <div className={styles.barInfo}>
        <p className={styles.barTitle} title={state.title}>
          {state.title}
        </p>
        <p className={styles.barSub} title={failed ? (state.error ?? "") : state.artist}>
          {failed ? (state.error ?? "Preview failed") : state.artist}
        </p>
      </div>
      {!failed && (
        <>
          <button
            type="button"
            className={styles.barBtn}
            onClick={() => void toggle()}
            disabled={loading}
            aria-label={state.status === "playing" ? "Pause preview" : "Resume preview"}
          >
            {loading ? (
              <span className={styles.spinner} aria-hidden="true" />
            ) : state.status === "playing" ? (
              <BiPause size={22} />
            ) : (
              <BiPlay size={24} />
            )}
          </button>
          <span className={styles.barTime}>{formatDurationSecs(state.position_ms / 1000)}</span>
          <input
            type="range"
            className={styles.barSeek}
            min={0}
            max={1000}
            value={Math.round(fraction * 1000)}
            disabled={loading || duration <= 0}
            aria-label="Preview position"
            onChange={(e) => void seek(Number(e.target.value) / 1000)}
          />
          <span className={styles.barTime}>
            {duration > 0 ? formatDurationSecs(duration / 1000) : "--:--"}
          </span>
        </>
      )}
      <button type="button" className={styles.barBtn} onClick={() => void stop()} aria-label="Close preview">
        <BiX size={20} />
      </button>
    </div>
  );
};
