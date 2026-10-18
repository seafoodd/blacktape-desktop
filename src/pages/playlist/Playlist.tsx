import { useCallback, useEffect } from "react";
import { usePlaylistStore } from "@/shared/store/playlistStore";
import styles from "./playlist.module.css";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useDominantColor } from "@/shared/hooks/useDominantColor.tsx";
import { FaCirclePause, FaCirclePlay, FaShuffle } from "react-icons/fa6";
import { BsThreeDots } from "react-icons/bs";
import clsx from "clsx";
import { MdEdit } from "react-icons/md";
import { Dropdown } from "@/shared/ui/dropdown";
import SongGrid from "@/features/library/ui/song-grid/SongGrid.tsx";
import { PlaylistCover } from "@/features/library/ui/playlists/PlaylistCover.tsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import DeletePlaylistModal from "@/features/library/ui/playlists/DeletePlaylistModal.tsx";
import { DropdownSeparator } from "@/shared/ui/dropdown/Dropdown.tsx";
import EditPlaylistModal from "@/features/library/ui/playlists/EditPlaylistModal.tsx";

const formatPlaylistStats = (trackCount: number, durationMs: number) => {
  const hours = Math.floor(durationMs / (1000 * 60 * 60));
  const minutes = Math.floor((durationMs % (1000 * 60 * 60)) / (1000 * 60));
  const timeString =
    hours > 0 ? `${hours} hr ${minutes} min` : `${minutes} min`;
  return `${trackCount} ${trackCount === 1 ? "song" : "songs"}, ${timeString}`;
};

const getCoverPath = (playlist?: {
  cover_url?: string | null;
  preview_covers?: string | string[] | null;
}) => {
  if (!playlist) return undefined;
  if (playlist.cover_url) return playlist.cover_url;
  const previews = playlist.preview_covers;
  if (typeof previews === "string" && previews.length > 0) {
    return previews.split(",")[0].trim();
  }
  if (Array.isArray(previews)) return previews[0];
  return undefined;
};

const Playlist = () => {
  const {
    activePlaylist,
    activePlaylistSongs,
    selectPlaylist,
    activePlaylistId,
    removeSongs,
    fetchPlaylists,
    reorderSongs,
  } = usePlaylistStore();

  const {
    startPlayback,
    isPlaying,
    shuffleMode,
    playbackContext,
    playbackContextId,
    togglePlay,
    toggleShuffle,
  } = useAudioStore();

  const { openModal } = useModalStore();

  const rawCover = getCoverPath(activePlaylist ?? undefined);
  const coverSrc = rawCover ? convertFileSrc(rawCover) : undefined;
  const dominantColor = useDominantColor(coverSrc);

  useEffect(() => {
    if (!activePlaylistId) return;

    selectPlaylist(activePlaylistId).catch((e) => {
      console.log("select playlist error:", e);
    });
  }, [activePlaylistId, selectPlaylist]);

  const handlePlaySong = useCallback(
    (clickedSongId: number) => {
      const currentIndex = activePlaylistSongs.findIndex(
        (song) => song.id === clickedSongId,
      );
      if (currentIndex === -1) return;

      const queueIds = activePlaylistSongs.map((s) => s.id);

      startPlayback(
        queueIds,
        currentIndex,
        "playlist",
        activePlaylistId ?? undefined,
      ).catch((e) => console.log("startPlayback error: ", e));
    },
    [activePlaylistSongs, activePlaylistId, startPlayback],
  );

  const isPlaylistPlaying =
    isPlaying &&
    playbackContext === "playlist" &&
    activePlaylistId === playbackContextId;

  // TODO: add saved shuffle setting for separate playlists and auto shuffle on playback start if it is enabled
  const isPlaylistShuffled = shuffleMode;

  const handlePlayButton = useCallback(() => {
    if (
      activePlaylistId !== playbackContextId &&
      activePlaylistSongs.length > 0
    ) {
      return handlePlaySong(activePlaylistSongs[0].id);
    }
    togglePlay();
  }, [
    activePlaylistId,
    playbackContextId,
    activePlaylistSongs,
    handlePlaySong,
    togglePlay,
  ]);

  const handleRemoveFromPlaylist = useCallback(
    (index: number) => {
      if (activePlaylistId === null) return;

      // Pass the index (position) instead of song.id
      removeSongs(activePlaylistId, [index])
        .catch((e) => console.log("remove song error: ", e))
        .finally(() => {
          fetchPlaylists();
          selectPlaylist(activePlaylistId);
        });
    },
    [activePlaylistId, removeSongs, fetchPlaylists],
  );

  const handleReorderSongs = useCallback(
    (fromIndex: number, toIndex: number) => {
      if (activePlaylistId === null) return;

      reorderSongs(activePlaylistId, fromIndex, toIndex)
        .catch((e) => console.log("reorder songs error: ", e))
        .finally(() => {
          fetchPlaylists();
          selectPlaylist(activePlaylistId);
        });
    },
    [activePlaylistId, reorderSongs],
  );

  if (!activePlaylist || !activePlaylistId) return;

  return (
    <div className={styles.container}>
      <div
        className={styles.playlistInfoBlock}
        style={{
          background: `linear-gradient(to bottom, rgba(${dominantColor}, 0.65), var(--color-background))`,
        }}
      >
        <div className={styles.playlistContainer}>
          <PlaylistCover
            coverUrl={activePlaylist.cover_url}
            previewCovers={activePlaylist.preview_covers}
            className={styles.playlistImage}
          />
          <div className={styles.playlistInfo}>
            <span className={styles.playlistText}>Playlist</span>
            <span className={clsx(styles.playlistName, "truncate")}>
              {activePlaylist.name}
            </span>
            <p className={clsx(styles.playlistDescription, "truncate")}>
              {activePlaylist.description}
            </p>
            <span className={styles.playlistStats}>
              {formatPlaylistStats(
                activePlaylist.track_count,
                activePlaylist.total_duration_ms,
              )}
            </span>
          </div>
        </div>
        <div className={styles.playlistControls}>
          <button className={styles.controlsButton} onClick={handlePlayButton}>
            {isPlaylistPlaying ? (
              <FaCirclePause
                className={styles.controlsButtonIconPlay}
                size={48}
              />
            ) : (
              <FaCirclePlay
                className={styles.controlsButtonIconPlay}
                size={48}
              />
            )}
          </button>
          <button
            className={clsx(
              styles.controlsButton,
              styles.shuffle,
              isPlaylistShuffled && styles.active,
            )}
            onClick={toggleShuffle}
            // disabled={activePlaylistId !== playbackContextId}
          >
            <FaShuffle className={styles.controlsButtonIcon} size={32} />
          </button>
          <button
            className={styles.controlsButton}
            onClick={() => {
              openModal(
                <EditPlaylistModal
                  playlistId={activePlaylistId}
                  name={activePlaylist.name}
                  description={activePlaylist.description}
                  onSuccess={fetchPlaylists}
                />,
              );
            }}
          >
            <MdEdit className={styles.controlsButtonIcon} size={32} />
          </button>

          <Dropdown.Root
            className={styles.controlsButton}
            trigger={
              <BsThreeDots className={styles.controlsButtonIcon} size={32} />
            }
          >
            <Dropdown.Item onClick={() => {}}>Add To Queue</Dropdown.Item>
            <DropdownSeparator />
            <Dropdown.Item onClick={() => {}}>Export</Dropdown.Item>
            <Dropdown.Item onClick={() => {}}>Edit</Dropdown.Item>
            <DropdownSeparator />
            <Dropdown.Item
              onClick={() => {
                openModal(
                  <DeletePlaylistModal playlistId={activePlaylistId} />,
                );
              }}
            >
              Delete
            </Dropdown.Item>
          </Dropdown.Root>
        </div>
      </div>
      <div className={styles.songList}>
        <SongGrid
          songs={activePlaylistSongs}
          onPlay={handlePlaySong}
          playbackContext={"playlist"}
          contextId={playbackContextId ?? undefined}
          onRemoveFromPlaylist={handleRemoveFromPlaylist}
          onReorderSongs={handleReorderSongs}
        />
      </div>
    </div>
  );
};

export default Playlist;
