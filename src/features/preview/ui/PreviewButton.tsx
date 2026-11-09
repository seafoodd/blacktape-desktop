import { useEffect, useRef, type MouseEvent } from "react";
import clsx from "clsx";
import { BiPause, BiPlay } from "react-icons/bi";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  isPreviewing,
  PREFETCH_HOVER_MS,
  usePreviewStore,
} from "../model/previewStore.ts";
import styles from "./preview.module.css";

interface PreviewButtonProps {
  item: SearchSuggestion;
  /** Extra class of the round button (placement over a cover). */
  className?: string;
}

/** Round play / pause / spinner button over a cover. Hovering warms the stream url. */
export const PreviewButton = ({ item, className }: PreviewButtonProps) => {
  const state = usePreviewStore((s) => s.state);
  const play = usePreviewStore((s) => s.play);
  const prefetch = usePreviewStore((s) => s.prefetch);
  const timer = useRef<number | undefined>(undefined);
  const active = isPreviewing(state, item.item_url_path);
  const loading = active && state.status === "loading";
  const playing = active && state.status === "playing";

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const onEnter = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => prefetch(item), PREFETCH_HOVER_MS);
  };
  const onLeave = () => window.clearTimeout(timer.current);
  const onClick = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    void play(item);
  };

  return (
    <button
      type="button"
      className={clsx(styles.button, active && styles.active, className)}
      aria-label={playing ? `Pause preview of ${item.name}` : `Preview ${item.name}`}
      aria-busy={loading}
      title={playing ? "Pause preview" : "Preview"}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onFocus={onEnter}
      onBlur={onLeave}
      onClick={onClick}
    >
      {loading ? (
        <span className={styles.spinner} aria-hidden="true" />
      ) : playing ? (
        <BiPause size={20} />
      ) : (
        <BiPlay size={22} />
      )}
    </button>
  );
};
