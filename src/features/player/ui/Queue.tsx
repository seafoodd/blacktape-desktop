import { useAudioStore } from "@/shared/store/audioStore.ts";
import { coverSrc } from "@/shared/lib/cover";
import styles from "./queue.module.css";
import placeholderSongImage from "@/assets/react.svg";
import clsx from "clsx";
import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  AddNextToQueue,
  AddToQueue,
  moveInQueue,
  playSongInQueue,
  removeFromQueue,
} from "@/shared/lib/audio.ts";
import { findPlayingRow } from "@/shared/lib/playingRow.ts";
import {
  EMPTY_SELECTION,
  nextSelection,
  pruneSelection,
  selectionForContextMenu,
  type SelectionState,
} from "@/shared/lib/selection.ts";
import {
  ContextMenu,
  ContextMenuPosition,
  MenuItemOption,
} from "@/shared/ui/context-menu";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { MdClose, MdPause, MdPlayArrow } from "react-icons/md";

interface QueueProps {
  className?: string;
}

const Queue = ({ className }: QueueProps) => {
  const { queue, currentSong, queueCursor, isPlaying, togglePlay } =
    useAudioStore();
  const { openModal } = useModalStore();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const activeItemRef = useRef<HTMLDivElement | null>(null);

  // Only one row is current even if the song is queued several times.
  const playingIndex = findPlayingRow(queue, currentSong?.id, queueCursor);

  // Selection is by position in the queue (the same song can be queued many times).
  const positions = useMemo(() => queue.map((_, i) => i), [queue.length]);
  const [selection, setSelection] = useState<SelectionState>(EMPTY_SELECTION);
  const [menu, setMenu] = useState<{
    position: ContextMenuPosition;
    items: MenuItemOption[];
  } | null>(null);
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dropTarget, setDropTarget] = useState<{
    index: number;
    position: "top" | "bottom";
  } | null>(null);

  // Positions past the end are gone after the queue shrinks.
  useEffect(() => {
    setSelection((prev) => pruneSelection(prev, positions));
  }, [positions]);

  useEffect(() => {
    const container = containerRef.current;
    const activeItem = activeItemRef.current;

    if (container && activeItem) {
      const containerRect = container.getBoundingClientRect();
      const itemRect = activeItem.getBoundingClientRect();

      // Check if the item is fully inside the visible area
      const isVisible =
        itemRect.top >= containerRect.top &&
        itemRect.bottom <= containerRect.bottom;

      if (!isVisible) {
        activeItem.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
      }
    }
  }, [currentSong?.id, playingIndex]);

  const removePositions = (toRemove: number[]) => {
    setSelection(EMPTY_SELECTION);
    removeFromQueue(toRemove).catch((e) =>
      console.error("removeFromQueue error: ", e),
    );
  };

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setSelection(EMPTY_SELECTION);
      } else if (e.key === "Delete" && selection.selected.size > 0) {
        const target = e.target as HTMLElement | null;
        if (target && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName)) return;
        removePositions([...selection.selected]);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  const { getContextMenuItems } = useSongContextMenu({
    onPlay: (_song, index) => {
      if (index !== undefined) handlePlaySong(index);
    },
    playbackContext: "library",
    onRemoveFromPlaylist: (index) => removePositions([index]),
    removeLabel: "Remove from Queue",
  });

  function handlePlaySong(index: number) {
    playSongInQueue(index).catch((e) =>
      console.log("startPlayback error: ", e),
    );
  }

  const handleSelect = (index: number, e: React.MouseEvent) => {
    setSelection((prev) =>
      nextSelection(prev, positions, index, {
        ctrl: e.ctrlKey || e.metaKey,
        shift: e.shiftKey,
      }),
    );
  };

  const multiMenuItems = (indices: number[]): MenuItemOption[] => {
    const ids = indices.map((i) => queue[i].id);
    return [
      {
        label: "Play Next",
        action: async () => {
          try {
            // Each one goes right after the current track: insert backwards.
            for (const id of [...ids].reverse()) await AddNextToQueue(id);
          } catch (e) {
            console.error("Play Next error: ", e);
          }
        },
      },
      {
        label: "Add to Queue",
        action: async () => {
          try {
            for (const id of ids) await AddToQueue(id);
          } catch (e) {
            console.error("Add to Queue error: ", e);
          }
        },
      },
      { separator: true, label: "" },
      {
        label: "Add to Playlist...",
        action: () => openModal(<AddToPlaylistModal songIds={ids} />),
      },
      { separator: true, label: "" },
      {
        label: `Remove ${indices.length} from Queue`,
        action: () => removePositions(indices),
      },
    ];
  };

  const handleOpenMenu = (index: number, position: ContextMenuPosition) => {
    const next = selectionForContextMenu(selection, index);
    setSelection(next);
    const indices = [...next.selected].sort((a, b) => a - b);
    setMenu({
      position,
      items:
        indices.length > 1
          ? multiMenuItems(indices)
          : getContextMenuItems(queue[index], index, index === playingIndex),
    });
  };

  const handleDragStart = (e: React.DragEvent, index: number) => {
    setDraggedIndex(index);
    e.dataTransfer.effectAllowed = "move";

    const count = selection.selected.has(index) ? selection.selected.size : 1;
    if (count > 1) {
      const ghost = document.createElement("div");
      ghost.textContent = `${count} tracks`;
      ghost.style.cssText =
        "position:absolute;top:-9999px;left:-9999px;padding:6px 12px;border-radius:8px;" +
        "background:rgba(18,18,18,0.95);color:#fff;font-size:13px;font-weight:600;";
      document.body.appendChild(ghost);
      e.dataTransfer.setDragImage(ghost, 10, 10);
      requestAnimationFrame(() => document.body.removeChild(ghost));
    }
  };

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const isBottom = e.clientY >= rect.top + rect.height / 2;
    setDropTarget({ index, position: isBottom ? "bottom" : "top" });
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const from = draggedIndex;
    const target = dropTarget;
    setDraggedIndex(null);
    setDropTarget(null);
    if (from === null || !target) return;

    const insertBefore = target.index + (target.position === "bottom" ? 1 : 0);
    // Dragging a selected row moves the whole selection; otherwise just that row.
    const moving =
      selection.selected.has(from) && selection.selected.size > 1
        ? [...selection.selected].sort((a, b) => a - b)
        : [from];

    setSelection(EMPTY_SELECTION);
    moveInQueue(moving, insertBefore).catch((err) =>
      console.error("moveInQueue error: ", err),
    );
  };

  const handleDragEnd = () => {
    setDraggedIndex(null);
    setDropTarget(null);
  };

  if (queue.length < 1) {
    return (
      <div className={clsx(styles.container, className)}>Queue is empty.</div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={clsx(styles.container, className)}
      onClick={(e) => {
        // A click outside the rows clears the selection.
        if (!(e.target as HTMLElement).closest("[data-queue-row]")) {
          setSelection(EMPTY_SELECTION);
        }
      }}
    >
      {queue.map((song, index) => {
        const isCurrent = index === playingIndex;
        const selected = selection.selected.has(index);
        const selectedAbove = selection.selected.has(index - 1);
        const selectedBelow = selection.selected.has(index + 1);

        return (
          <div
            key={`${song.id}-${index}`}
            ref={isCurrent ? activeItemRef : null}
            data-queue-row
            className={clsx(styles.item, {
              [styles.active]: isCurrent,
              [styles.selected]: selected,
              [styles.selectedTop]: selected && !selectedAbove,
              [styles.selectedBottom]: selected && !selectedBelow,
              [styles.selectedJoined]: selected && selectedBelow,
              // Dragging a selected row drags the whole selection, so dim all of it.
              [styles.dragging]:
                draggedIndex !== null &&
                (draggedIndex === index ||
                  (selected && selection.selected.has(draggedIndex))),
              [styles.dropTop]:
                dropTarget?.index === index && dropTarget.position === "top",
              [styles.dropBottom]:
                dropTarget?.index === index && dropTarget.position === "bottom",
            })}
            aria-selected={selected}
            draggable
            onClick={(e) => handleSelect(index, e)}
            onDoubleClick={() => handlePlaySong(index)}
            onContextMenu={(e) => {
              e.preventDefault();
              handleOpenMenu(index, { x: e.clientX, y: e.clientY });
            }}
            onDragStart={(e) => handleDragStart(e, index)}
            onDragOver={(e) => handleDragOver(e, index)}
            onDrop={handleDrop}
            onDragEnd={handleDragEnd}
          >
            <div className={styles.coverWrap}>
              <img
                loading="lazy"
                decoding="async"
                className={styles.songCover}
                src={
                  song.cover_url
                    ? coverSrc(song.cover_url, 48)
                    : placeholderSongImage
                }
                alt={song.title}
              />
              <button
                className={styles.playButton}
                title={isCurrent && isPlaying ? "Pause" : "Play"}
                aria-label={isCurrent && isPlaying ? "Pause" : "Play"}
                onClick={(e) => {
                  e.stopPropagation();
                  if (isCurrent) {
                    togglePlay().catch((err) =>
                      console.error("togglePlay error: ", err),
                    );
                  } else {
                    handlePlaySong(index);
                  }
                }}
                onDoubleClick={(e) => e.stopPropagation()}
              >
                {isCurrent && isPlaying ? (
                  <MdPause size={24} />
                ) : (
                  <MdPlayArrow size={24} className={styles.playIcon} />
                )}
              </button>
            </div>
            <div className={styles.info}>
              <span className={styles.title}>{song.title}</span>
              <span className={styles.artist}>
                {Array.isArray(song.artists)
                  ? song.artists.join(", ")
                  : song.artists}
              </span>
            </div>
            <button
              className={styles.removeButton}
              title="Remove from queue"
              aria-label="Remove from queue"
              onClick={(e) => {
                e.stopPropagation();
                removePositions([index]);
              }}
            >
              <MdClose size={18} />
            </button>
          </div>
        );
      })}
      {menu && (
        <ContextMenu
          position={menu.position}
          items={menu.items}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
};

export default Queue;
