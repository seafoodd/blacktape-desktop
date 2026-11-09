import { useEffect, useRef, type MouseEvent } from "react";
import clsx from "clsx";
import { MdPause, MdPlayArrow } from "react-icons/md";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import {
  isStreamingUrl,
  PREFETCH_HOVER_MS,
  useStreamStore,
} from "../model/streamStore.ts";
import styles from "./stream-button.module.css";

interface StreamButtonProps {
  item: SearchSuggestion;
  /** The list the item belongs to: the tracks after it are queued behind it. */
  list?: readonly SearchSuggestion[];
  /** Placement class of the button (it sits over the cover, like the play button of the queue). */
  className?: string;
}

/**
 * Play / pause / spinner button over a cover. Starts the streaming playback of the track in the normal player;
 * hovering warms the stream url.
 */
export const StreamButton = ({ item, list, className }: StreamButtonProps) => {
  const url = item.item_url_path;
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);
  const streamLoading = useAudioStore((s) => s.streamLoading);
  const pending = useStreamStore((s) => s.pendingUrl === url);
  const error = useStreamStore((s) => (s.error?.url === url ? s.error.message : null));
  const play = useStreamStore((s) => s.play);
  const prefetch = useStreamStore((s) => s.prefetch);
  const timer = useRef<number | undefined>(undefined);
  const active = isStreamingUrl(currentSong, isStreaming, url);
  const loading = pending || (active && streamLoading);
  const playing = active && isPlaying && !streamLoading;

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const onEnter = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => prefetch(item), PREFETCH_HOVER_MS);
  };
  const onLeave = () => window.clearTimeout(timer.current);
  const onClick = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    void play(item, list);
  };

  const label = playing ? "Pause" : "Play preview";
  return (
    <button
      type="button"
      className={clsx(
        styles.button,
        (active || loading || error) && styles.alwaysVisible,
        className,
      )}
      title={error ?? label}
      aria-label={`${label}: ${item.name}`}
      aria-busy={loading}
      onMouseEnter={onEnter}
      onMouseLeave={onLeave}
      onFocus={onEnter}
      onBlur={onLeave}
      onClick={onClick}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      {loading ? (
        <span className={styles.spinner} aria-hidden="true" />
      ) : error ? (
        <span className={styles.errorMark} aria-hidden="true">
          !
        </span>
      ) : playing ? (
        <MdPause size={24} />
      ) : (
        <MdPlayArrow size={24} className={styles.playIcon} />
      )}
    </button>
  );
};
