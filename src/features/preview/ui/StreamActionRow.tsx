import clsx from "clsx";
import { MdPlayArrow, MdPlaylistAdd, MdShuffle } from "react-icons/md";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useStreamStore } from "../model/streamStore.ts";
import styles from "./stream-action-row.module.css";

export interface StreamActionRowProps {
  /** Tracks the buttons act on (stream order). The row renders nothing for an empty list. */
  items: readonly SearchSuggestion[];
  /** Label of the play button: "Play" (default) or "Play album". */
  playLabel?: string;
  className?: string;
  /** Optional local-aware action overrides (e.g. AlbumPreview mixing local + streamed tracks). */
  onPlayAll?: () => void;
  onEnqueue?: () => void;
  onShuffle?: () => void;
}

/**
 * "Play" / "Add to queue" / "Shuffle" buttons that stream `items` through `useStreamStore.playAll/enqueue`.
 */
export const StreamActionRow = ({
  items,
  playLabel = "Play",
  className,
  onPlayAll,
  onEnqueue,
  onShuffle,
}: StreamActionRowProps) => {
  const playAll = useStreamStore((s) => s.playAll);
  const enqueue = useStreamStore((s) => s.enqueue);
  if (items.length === 0) return null;

  return (
    <div className={clsx(styles.row, className)} role="group" aria-label="Stream actions">
      <button
        type="button"
        className={styles.button}
        onClick={() => (onPlayAll ? onPlayAll() : void playAll(items))}
      >
        <MdPlayArrow size={18} aria-hidden="true" />
        <span>{playLabel}</span>
      </button>
      <button
        type="button"
        className={styles.button}
        title="Add these tracks to the queue"
        onClick={() => (onEnqueue ? onEnqueue() : void enqueue(items))}
      >
        <MdPlaylistAdd size={18} aria-hidden="true" />
        <span>Add to queue</span>
      </button>
      <button
        type="button"
        className={styles.button}
        title="Play these tracks in random order"
        onClick={() => (onShuffle ? onShuffle() : void playAll(items, { shuffle: true }))}
      >
        <MdShuffle size={18} aria-hidden="true" />
        <span>Shuffle</span>
      </button>
    </div>
  );
};
