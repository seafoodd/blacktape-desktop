import clsx from "clsx";
import styles from "./song-grid.module.css";
import { formatDuration } from "@/shared/lib/time.ts";
import { PlaybackContext, useAudioStore } from "@/shared/store/audioStore.ts";
import { Song } from "@/shared/types.ts";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";
import React, { useState } from "react";
import { ContextMenu, ContextMenuPosition } from "@/shared/ui/context-menu";
import { HiDotsHorizontal, HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { convertFileSrc } from "@tauri-apps/api/core";

interface SongGridProps {
  songs: Song[];
  onPlay: (clickedSongId: number) => void;
  playbackContext: PlaybackContext;
  contextId?: number;
  onRemoveFromPlaylist?: (index: number) => void;
  onReorderSongs?: (fromIndex: number, toIndex: number) => void;
}

export const SongGrid = ({
  songs,
  onPlay,
  playbackContext,
  contextId,
  onRemoveFromPlaylist,
  onReorderSongs,
}: SongGridProps) => {
  const { currentSong, isPlaying } = useAudioStore();
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dropTarget, setDropTarget] = useState<{
    index: number;
    position: "top" | "bottom";
  } | null>(null);

  const handleDragStart = (e: React.DragEvent, index: number) => {
    setDraggedIndex(index);
    e.dataTransfer.effectAllowed = "move";
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

    const updatedSongs = [...songs];
    const [movedSong] = updatedSongs.splice(draggedIndex, 1);
    updatedSongs.splice(insertIndex, 0, movedSong);

    setDraggedIndex(null);
    setDropTarget(null);
    onReorderSongs(draggedIndex, insertIndex);
  };

  const handleDragEnd = () => {
    setDraggedIndex(null);
    setDropTarget(null);
  };

  return (
    <div className={styles.trackTable}>
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
            currentSong={currentSong}
            isPlaying={isPlaying}
            playbackContext={playbackContext}
            contextId={contextId}
            onPlay={onPlay}
            onRemoveFromPlaylist={
              onRemoveFromPlaylist
                ? () => onRemoveFromPlaylist(index)
                : undefined
            }
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
    </div>
  );
};

interface SongGridRowProps {
  song: Song;
  index: number;
  currentSong: Song | null;
  isPlaying: boolean;
  playbackContext: PlaybackContext;
  contextId?: number;
  onPlay: (id: number) => void;
  onRemoveFromPlaylist?: (index: number) => void;
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
  currentSong,
  isPlaying,
  playbackContext,
  contextId,
  onPlay,
  onRemoveFromPlaylist,
  draggable,
  isDragged,
  dropTarget,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
}: SongGridRowProps) => {
  const { getContextMenuItems } = useSongContextMenu({
    onPlay: (s) => onPlay(s.id),
    playbackContext,
    contextId,
    onRemoveFromPlaylist,
  });

  const [activeMenu, setActiveMenu] = useState<{
    position: ContextMenuPosition;
    song: Song;
    index: number;
  } | null>(null);

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setActiveMenu({ position: { x: e.clientX, y: e.clientY }, song, index });
  };

  const handleOpenContextMenu = (
    e: React.MouseEvent,
    song: Song,
    fromThreeDots = false,
  ) => {
    e.preventDefault();
    e.stopPropagation();

    let x = e.clientX;
    let y = e.clientY;

    if (fromThreeDots) {
      const rect = e.currentTarget.getBoundingClientRect();
      x = rect.left;
      y = rect.bottom + 4;
    }

    setActiveMenu({ position: { x, y }, song, index });
  };

  const isCurrent = currentSong && song.id === currentSong.id;
  const coverSrc = song.cover_url ? convertFileSrc(song.cover_url) : undefined;

  const isTopTarget =
    dropTarget?.index === index && dropTarget.position === "top";
  const isBottomTarget =
    dropTarget?.index === index && dropTarget.position === "bottom";

  return (
    <>
      <div
        className={clsx(styles.songRow, {
          [styles.playing]: isCurrent,
          [styles.dragging]: isDragged,
          [styles.dropTargetTop]: isTopTarget,
          [styles.dropTargetBottom]: isBottomTarget,
        })}
        onClick={() => onPlay(song.id)}
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
          {coverSrc && (
            <img src={coverSrc} className={styles.songCover} alt="" />
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
            onClick={(e) => handleOpenContextMenu(e, song, true)}
            aria-label="Song options"
          >
            <HiDotsHorizontal className={styles.threeDotIcon} size={24} />
          </button>
        </div>
      </div>

      {activeMenu && (
        <ContextMenu
          position={activeMenu.position}
          items={getContextMenuItems(activeMenu.song, activeMenu.index)}
          onClose={() => setActiveMenu(null)}
        />
      )}
    </>
  );
};

export default SongGrid;
