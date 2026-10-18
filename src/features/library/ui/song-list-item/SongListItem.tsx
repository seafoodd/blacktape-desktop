import clsx from "clsx";
import styles from "./song-list-item.module.css";
import { HiDotsHorizontal, HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { formatDuration } from "@/shared/lib/time.ts";
import React, { useState } from "react";
import { PlaybackContext, useAudioStore } from "@/shared/store/audioStore.ts";
import { Song } from "@/shared/types.ts";
import { ContextMenu, ContextMenuPosition } from "@/shared/ui/context-menu";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";

interface SongListItemProps {
  song: Song;
  onPlay: (clickedSongId: number) => void;
  playbackContext: PlaybackContext;
  contextId?: number;
  index?: number;
}

const SongListItem = ({
  song,
  onPlay,
  playbackContext,
  contextId,
  index,
}: SongListItemProps) => {
  const { currentSong, isPlaying } = useAudioStore();
  const { getContextMenuItems } = useSongContextMenu({
    onPlay: (song) => onPlay(song.id),
    playbackContext,
    contextId,
  });

  const [activeMenu, setActiveMenu] = useState<{
    position: ContextMenuPosition;
    song: Song;
  } | null>(null);

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

    setActiveMenu({
      position: { x, y },
      song,
    });
  };

  const handleDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData(
      "application/json",
      JSON.stringify({ songId: song.id }),
    );
    e.dataTransfer.effectAllowed = "copy";

    const ghost = document.createElement("div");
    ghost.style.position = "absolute";
    ghost.style.top = "-9999px";
    ghost.style.left = "-9999px";
    ghost.style.display = "flex";
    ghost.style.alignItems = "center";
    ghost.style.gap = "10px";
    ghost.style.padding = "8px 12px";
    ghost.style.background = "rgba(18, 18, 18, 0.95)";
    ghost.style.backdropFilter = "blur(12px)";
    ghost.style.border = "1px solid rgba(255, 255, 255, 0.12)";
    ghost.style.borderRadius = "8px";
    ghost.style.boxShadow = "0 12px 32px rgba(0, 0, 0, 0.6)";
    ghost.style.color = "#ffffff";
    ghost.style.fontFamily = "inherit";
    ghost.style.width = "200px";
    ghost.style.pointerEvents = "none";
    ghost.style.zIndex = "9999";

    // Thumbnail
    const thumb = document.createElement("div");
    thumb.style.width = "36px";
    thumb.style.height = "36px";
    thumb.style.borderRadius = "4px";
    thumb.style.background = "var(--color-primary, #1db954)";
    thumb.style.display = "flex";
    thumb.style.alignItems = "center";
    thumb.style.justifyContent = "center";
    thumb.style.flexShrink = "0";
    thumb.innerHTML = `<svg stroke="currentColor" fill="currentColor" stroke-width="0" viewBox="0 0 24 24" height="20" width="20" xmlns="http://www.w3.org/2000/svg"><path d="M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z"></path></svg>`;
    ghost.appendChild(thumb);

    // Text container
    const textContainer = document.createElement("div");
    textContainer.style.display = "flex";
    textContainer.style.flexDirection = "column";
    textContainer.style.minWidth = "0";
    textContainer.style.flex = "1";

    const titleEl = document.createElement("span");
    titleEl.textContent = song.title;
    titleEl.style.fontSize = "13px";
    titleEl.style.fontWeight = "600";
    titleEl.style.whiteSpace = "nowrap";
    titleEl.style.overflow = "hidden";
    titleEl.style.textOverflow = "ellipsis";
    textContainer.appendChild(titleEl);

    const actionEl = document.createElement("span");
    actionEl.textContent = "Drop to add";
    actionEl.style.fontSize = "11px";
    actionEl.style.color = "var(--color-primary, #1db954)";
    actionEl.style.fontWeight = "500";
    textContainer.appendChild(actionEl);

    ghost.appendChild(textContainer);

    document.body.appendChild(ghost);
    e.dataTransfer.setDragImage(ghost, 20, 20);

    requestAnimationFrame(() => {
      document.body.removeChild(ghost);
    });
  };

  return (
    <div
      className={clsx(styles.song, {
        [styles.playing]: currentSong && song.id == currentSong.id,
      })}
      onClick={() => onPlay(song.id)}
      onContextMenu={(e) => handleOpenContextMenu(e, song)}
      onDragStart={handleDragStart}
      draggable
      key={song.id}
      role="button"
      tabIndex={0}
    >
      <div className={styles.songLeft}>
        {currentSong && song.id == currentSong.id ? (
          isPlaying ? (
            <HiOutlineMusicNote className={styles.playingIcon} />
          ) : (
            <HiMiniPause className={styles.pauseIcon} />
          )
        ) : (
          <div className={styles.songTrackNumber}>
            {index !== undefined
              ? String(index + 1).padStart(2, "0")
              : song.track_number && String(song.track_number).padStart(2, "0")}
          </div>
        )}
        <div className={clsx(styles.songTitle, "truncate")}>{song.title}</div>
      </div>
      <div className={styles.songRight}>
        <div className={styles.songDuration}>
          {formatDuration(song.duration_ms)}
        </div>
        <button
          className={styles.threeDotButton}
          onClick={(e) => handleOpenContextMenu(e, song, true)}
          aria-label="Song options"
        >
          <HiDotsHorizontal className={styles.threeDotIcon} size={24} />
        </button>
      </div>
      {activeMenu && (
        <ContextMenu
          position={activeMenu.position}
          items={getContextMenuItems(activeMenu.song)}
          onClose={() => setActiveMenu(null)}
        />
      )}
    </div>
  );
};

export default SongListItem;
