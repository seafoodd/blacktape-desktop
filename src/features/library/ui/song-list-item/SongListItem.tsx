import clsx from "clsx";
import styles from "./song-list-item.module.css";
import { HiDotsHorizontal, HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { sameUrl } from "@/shared/lib/urlKey.ts";
import { MdPause, MdPlayArrow } from "react-icons/md";
import { formatDuration } from "@/shared/lib/time.ts";
import React from "react";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { Song } from "@/shared/types.ts";
import { ContextMenuPosition } from "@/shared/ui/context-menu";
import { SONG_DRAG_MIME, encodeSongDrag } from "@/shared/lib/dragSongs.ts";
import { QualityBadge } from "@/shared/ui/quality-badge/QualityBadge.tsx";

interface SongListItemProps {
  song: Song;
  /** Double click: start playing this song. */
  onPlay: (clickedSongId: number) => void;
  /** Single click: the parent updates the selection (Ctrl/Shift aware). */
  onSelect: (song: Song, e: React.MouseEvent) => void;
  /** Right click or the three-dots button: the parent builds and shows the menu. */
  onOpenMenu: (song: Song, position: ContextMenuPosition) => void;
  selected: boolean;
  /** Whether the neighbours above/below are selected too (to draw one frame per group). */
  selectedAbove?: boolean;
  selectedBelow?: boolean;
  /** Songs a drag from this row carries: the whole selection if the row is part of it. */
  dragIds: number[];
  index?: number;
}

const SongListItem = ({
  song,
  onPlay,
  onSelect,
  onOpenMenu,
  selected,
  selectedAbove,
  selectedBelow,
  dragIds,
  index,
}: SongListItemProps) => {
  const { currentSong, isPlaying, isStreaming, togglePlay } = useAudioStore();
  // A song that was streamed (before it was downloaded) is the current one too while its stream plays.
  const isCurrent =
    !!currentSong &&
    (song.id === currentSong.id ||
      (isStreaming && sameUrl(song.source_url, currentSong.source_url)));

  const handleOpenContextMenu = (
    e: React.MouseEvent,
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

    onOpenMenu(song, { x, y });
  };

  const handleDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData(SONG_DRAG_MIME, encodeSongDrag(dragIds));
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
    titleEl.textContent =
      dragIds.length > 1 ? `${dragIds.length} tracks` : song.title;
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
        [styles.selected]: selected,
        [styles.selectedTop]: selected && !selectedAbove,
        [styles.selectedBottom]: selected && !selectedBelow,
      })}
      data-song-row
      aria-selected={selected}
      onClick={(e) => onSelect(song, e)}
      onDoubleClick={() => onPlay(song.id)}
      onContextMenu={(e) => handleOpenContextMenu(e)}
      onDragStart={handleDragStart}
      draggable
      key={song.id}
      role="button"
      tabIndex={0}
    >
      <div className={styles.songLeft}>
        <div className={styles.numSlot}>
          {/* Hidden on hover: the play button takes its place. */}
          <div className={styles.numContent}>
            {isCurrent ? (
              isPlaying ? (
                <HiOutlineMusicNote className={styles.playingIcon} />
              ) : (
                <HiMiniPause className={styles.pauseIcon} />
              )
            ) : (
              <div className={styles.songTrackNumber}>
                {index !== undefined
                  ? String(index + 1).padStart(2, "0")
                  : song.track_number &&
                    String(song.track_number).padStart(2, "0")}
              </div>
            )}
          </div>
          <button
            className={styles.rowPlayButton}
            title={isCurrent && isPlaying ? "Pause" : "Play"}
            aria-label={isCurrent && isPlaying ? "Pause" : "Play"}
            onClick={(e) => {
              e.stopPropagation();
              if (isCurrent) {
                togglePlay().catch((err) =>
                  console.error("togglePlay error: ", err),
                );
              } else {
                onPlay(song.id);
              }
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            {isCurrent && isPlaying ? (
              <MdPause size={22} />
            ) : (
              <MdPlayArrow size={22} className={styles.playIcon} />
            )}
          </button>
        </div>
        <div className={clsx(styles.songTitle, "truncate")}>{song.title}</div>
      </div>
      <div className={styles.songRight}>
        <QualityBadge song={song} className={styles.qualityBadge} />
        {/* the duration and the "..." button share one cell: hovering swaps them without moving the badge */}
        <div className={styles.endSlot}>
          <div className={styles.songDuration}>
            {formatDuration(song.duration_ms)}
          </div>
          <button
            className={styles.threeDotButton}
            onClick={(e) => handleOpenContextMenu(e, true)}
            aria-label="Song options"
          >
            <HiDotsHorizontal className={styles.threeDotIcon} size={24} />
          </button>
        </div>
      </div>
    </div>
  );
};

export default SongListItem;
