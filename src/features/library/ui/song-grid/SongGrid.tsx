import clsx from "clsx";
import styles from "./song-grid.module.css";
import { formatDuration } from "@/shared/lib/time.ts";
import { PlaybackContext, useAudioStore } from "@/shared/store/audioStore.ts";
import { Song } from "@/shared/types.ts";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";
import React, { useEffect, useMemo, useState } from "react";
import {
  ContextMenu,
  ContextMenuPosition,
  MenuItemOption,
} from "@/shared/ui/context-menu";
import { HiDotsHorizontal, HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { coverSrc } from "@/shared/lib/cover";
import {
  EMPTY_SELECTION,
  nextSelection,
  selectionForContextMenu,
  type SelectionState,
} from "@/shared/lib/selection.ts";
import { AddNextToQueue, AddToQueue } from "@/shared/lib/audio.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";

interface SongGridProps {
  songs: Song[];
  /** Double click / menu "Play": start playing the row at `index`. */
  onPlay: (index: number) => void;
  playbackContext: PlaybackContext;
  contextId?: number;
  /** The one row that is playing (a song can be in the list several times). */
  playingIndex: number | null;
  onRemoveFromPlaylist?: (index: number) => void;
  onRemoveManyFromPlaylist?: (indices: number[]) => void;
  onReorderSongs?: (fromIndex: number, toIndex: number) => void;
  /** Drag of a selected row: move all selected rows to before `insertAt` (original index). */
  onMoveSelection?: (indices: number[], insertAt: number) => void;
}

export const SongGrid = ({
  songs,
  onPlay,
  playbackContext,
  contextId,
  playingIndex,
  onRemoveFromPlaylist,
  onRemoveManyFromPlaylist,
  onReorderSongs,
  onMoveSelection,
}: SongGridProps) => {
  const { isPlaying, startPlayback } = useAudioStore();
  const { openModal } = useModalStore();
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dropTarget, setDropTarget] = useState<{
    index: number;
    position: "top" | "bottom";
  } | null>(null);

  // Selection is by row index, not song id: the same song can appear many times.
  const rowIndices = useMemo(() => songs.map((_, i) => i), [songs]);
  const [selection, setSelection] = useState<SelectionState>(EMPTY_SELECTION);
  const [menu, setMenu] = useState<{
    position: ContextMenuPosition;
    items: MenuItemOption[];
  } | null>(null);

  // Rows move when the list changes (reorder, remove, refresh), so indices go stale.
  useEffect(() => {
    setSelection(EMPTY_SELECTION);
  }, [songs]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSelection(EMPTY_SELECTION);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const { getContextMenuItems } = useSongContextMenu({
    onPlay: (_song, index) => {
      if (index !== undefined) onPlay(index);
    },
    playbackContext,
    contextId,
    onRemoveFromPlaylist,
  });

  const handleSelect = (index: number, e: React.MouseEvent) => {
    setSelection((prev) =>
      nextSelection(prev, rowIndices, index, {
        ctrl: e.ctrlKey || e.metaKey,
        shift: e.shiftKey,
      }),
    );
  };

  /** Menu for several selected rows. */
  const multiMenuItems = (indices: number[]): MenuItemOption[] => {
    const ids = indices.map((i) => songs[i].id);
    const label = `${indices.length} tracks`;
    const items: MenuItemOption[] = [
      {
        label: `Play ${label}`,
        action: () => {
          startPlayback(ids, 0, "library").catch((e) =>
            console.error("Play selection error: ", e),
          );
        },
      },
      {
        label: "Play Next",
        action: async () => {
          try {
            // Each one goes right after the current track, so insert backwards
            // to keep the selection order.
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
    ];
    if (onRemoveManyFromPlaylist) {
      items.push(
        { separator: true, label: "" },
        {
          label: "Remove from Playlist",
          action: () => onRemoveManyFromPlaylist(indices),
        },
      );
    }
    return items;
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
          : getContextMenuItems(songs[index], index, index === playingIndex),
    });
  };

  const handleDragStart = (e: React.DragEvent, index: number) => {
    setDraggedIndex(index);
    e.dataTransfer.effectAllowed = "move";

    // Dragging a selected row drags the whole selection: show a small badge.
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

    // Calculate if mouse is in the top or bottom half of the row
    const target = e.currentTarget as HTMLElement;
    const rect = target.getBoundingClientRect();
    const middleY = rect.top + rect.height / 2;
    const isBottom = e.clientY >= middleY;

    setDropTarget({ index, position: isBottom ? "bottom" : "top" });
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    if (draggedIndex === null || !onReorderSongs || !dropTarget) {
      setDraggedIndex(null);
      setDropTarget(null);
      return;
    }

    // 1. Determine the raw insertion index based on top/bottom hover
    let insertIndex = dropTarget.index;
    if (dropTarget.position === "bottom") {
      insertIndex += 1;
    }

    // Dragging one of several selected rows moves the whole selection as a block.
    if (
      onMoveSelection &&
      selection.selected.size > 1 &&
      selection.selected.has(draggedIndex)
    ) {
      const indices = [...selection.selected].sort((a, b) => a - b);
      setDraggedIndex(null);
      setDropTarget(null);
      onMoveSelection(indices, insertIndex);
      return;
    }

    // 2. Adjust for the array shift if dragging DOWN
    if (draggedIndex < insertIndex) {
      insertIndex -= 1;
    }

    // 3. Prevent dropping on itself (no-op)
    if (draggedIndex === insertIndex) {
      setDraggedIndex(null);
      setDropTarget(null);
      return;
    }

    setDraggedIndex(null);
    setDropTarget(null);
    onReorderSongs(draggedIndex, insertIndex);
  };

  const handleDragEnd = () => {
    setDraggedIndex(null);
    setDropTarget(null);
  };

  return (
    <div
      className={styles.trackTable}
      onClick={(e) => {
        // A click outside the song rows clears the selection.
        if (!(e.target as HTMLElement).closest("[data-song-row]")) {
          setSelection(EMPTY_SELECTION);
        }
      }}
    >
      <div className={styles.tableHeader}>
        <span className={styles.headerNum}>#</span>
        <span>Title</span>
        <span>Artist</span>
        <span>Album</span>
        <span className={styles.headerDuration}>Duration</span>
      </div>
      <div className={styles.songList}>
        {songs.map((song, index) => (
          <SongGridRow
            key={`${song.id}-${index}`}
            song={song}
            index={index}
            isCurrent={index === playingIndex}
            isPlaying={isPlaying}
            selected={selection.selected.has(index)}
            selectedAbove={selection.selected.has(index - 1)}
            selectedBelow={selection.selected.has(index + 1)}
            onSelect={handleSelect}
            onPlay={onPlay}
            onOpenMenu={handleOpenMenu}
            draggable={!!onReorderSongs}
            isDragged={draggedIndex === index}
            dropTarget={dropTarget}
            onDragStart={(e) => handleDragStart(e, index)}
            onDragOver={(e) => handleDragOver(e, index)}
            onDrop={handleDrop}
            onDragEnd={handleDragEnd}
          />
        ))}
      </div>
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

interface SongGridRowProps {
  song: Song;
  index: number;
  isCurrent: boolean;
  isPlaying: boolean;
  selected: boolean;
  selectedAbove: boolean;
  selectedBelow: boolean;
  onSelect: (index: number, e: React.MouseEvent) => void;
  onPlay: (index: number) => void;
  onOpenMenu: (index: number, position: ContextMenuPosition) => void;
  draggable?: boolean;
  isDragged?: boolean;
  dropTarget: { index: number; position: "top" | "bottom" } | null;
  onDragStart?: (e: React.DragEvent) => void;
  onDragOver?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent) => void;
  onDragEnd?: (e: React.DragEvent) => void;
}

const SongGridRow = ({
  song,
  index,
  isCurrent,
  isPlaying,
  selected,
  selectedAbove,
  selectedBelow,
  onSelect,
  onPlay,
  onOpenMenu,
  draggable,
  isDragged,
  dropTarget,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
}: SongGridRowProps) => {
  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    onOpenMenu(index, { x: e.clientX, y: e.clientY });
  };

  const handleThreeDots = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const rect = e.currentTarget.getBoundingClientRect();
    onOpenMenu(index, { x: rect.left, y: rect.bottom + 4 });
  };

  const songCoverSrc = song.cover_url ? coverSrc(song.cover_url, 48) : undefined;

  const isTopTarget =
    dropTarget?.index === index && dropTarget.position === "top";
  const isBottomTarget =
    dropTarget?.index === index && dropTarget.position === "bottom";

  return (
    <div
      className={clsx(styles.songRow, {
        [styles.playing]: isCurrent,
        [styles.selected]: selected,
        [styles.selectedTop]: selected && !selectedAbove,
        [styles.selectedBottom]: selected && !selectedBelow,
        [styles.dragging]: isDragged,
        [styles.dropTargetTop]: isTopTarget,
        [styles.dropTargetBottom]: isBottomTarget,
      })}
      data-song-row
      aria-selected={selected}
      onClick={(e) => onSelect(index, e)}
      onDoubleClick={() => onPlay(index)}
      onContextMenu={handleContextMenu}
      draggable={draggable}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onDragEnd={onDragEnd}
      role="button"
      tabIndex={0}
    >
      <div className={clsx(styles.cell, styles.cellNum)}>
        {isCurrent ? (
          isPlaying ? (
            <HiOutlineMusicNote className={styles.playingIcon} size={18} />
          ) : (
            <HiMiniPause className={styles.pauseIcon} size={18} />
          )
        ) : (
          String(index + 1).padStart(2, "0")
        )}
      </div>

      <div className={clsx(styles.cell, styles.cellTitle)}>
        {songCoverSrc && (
          <img
            loading="lazy"
            decoding="async"
            src={songCoverSrc}
            className={styles.songCover}
            alt=""
          />
        )}
        <span className="truncate">{song.title}</span>
      </div>

      <div
        className={clsx(
          styles.cell,
          { [styles.playing]: isCurrent },
          "truncate",
        )}
      >
        {song.artists || "—"}
      </div>

      <div
        className={clsx(
          styles.cell,
          { [styles.playing]: isCurrent },
          "truncate",
        )}
      >
        {song.album || "—"}
      </div>

      <div
        className={clsx(
          styles.cell,
          { [styles.playing]: isCurrent },
          styles.cellDuration,
        )}
      >
        <span className={styles.durationText}>
          {formatDuration(song.duration_ms)}
        </span>
        <button
          className={styles.threeDotButton}
          onClick={handleThreeDots}
          aria-label="Song options"
        >
          <HiDotsHorizontal className={styles.threeDotIcon} size={24} />
        </button>
      </div>
    </div>
  );
};

export default SongGrid;
