import { useEffect, useMemo, useRef, useState } from "react";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import styles from "./player-controls.module.css";
import {
  calculateDisplayProgress,
  progressToSliderValue,
  sliderValueToProgress,
} from "@/features/player/ui/seekProgress.ts";
import { DownloadSplitButton } from "@/shared/ui/download-split-button/DownloadSplitButton.tsx";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import { formatDuration } from "@/shared/lib/ui/time.ts";
import {
  MdCloudQueue,
  MdCheck,
  MdFavorite,
  MdFavoriteBorder,
  MdFileDownload,
  MdPlaylistAdd,
  MdPause,
  MdPlayArrow,
  MdRepeat,
  MdRepeatOne,
  MdShuffle,
  MdSkipNext,
  MdSkipPrevious,
  MdVolumeOff,
  MdVolumeUp,
} from "react-icons/md";
import clsx from "clsx";
import { coverSrc } from "@/shared/lib/remote/cover";
import placeholderSongImage from "@/assets/react.svg";
import { useFavoritesStore } from "@/features/favorites/model/favoritesStore.ts";
import {
  ItemType,
  useLibraryStore,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { remoteTrackDownloadParams } from "@/features/remote-track-menu/model/buildRemoteTrackMenu.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { useTrackStatus } from "@/shared/hooks/useDownloadStatus.ts";
import { findDownload, isRunning } from "@/shared/lib/library/downloadMatch.ts";
import { streamAlbumUrl } from "@/shared/lib/remote/stream.ts";
import {
  albumTarget,
  remoteTrackRef,
  streamSongToRemoteTrack,
} from "../lib/currentSong.ts";
import { RepeatMode } from "@/shared/types.ts";

export const PlayerControls = () => {
  const {
    currentSong,
    isStreaming,
    streamLoading,
    progress,
    volume,
    setVolume,
    isPlaying,
    togglePlay,
    setProgress,
    seek: storeSeek,
    next,
    previous,
    updateProgress,
    shuffleMode,
    toggleShuffle,
    cycleRepeatMode,
    repeatMode,
    playbackError,
  } = useAudioStore();
  const [isDragging, setIsDragging] = useState<boolean>(false);
  const [dragProgress, setDragProgress] = useState<number | null>(null);
  const isDraggingRef = useRef<boolean>(false);
  const dragProgressRef = useRef<number | null>(null);
  const [mutedVolume, setMutedVolume] = useState<number>(volume);

  useEffect(() => {
    isDraggingRef.current = false;
    dragProgressRef.current = null;
    setIsDragging(false);
    setDragProgress(null);
  }, [currentSong?.id, currentSong?.source_url]);

  useEffect(() => {
    if (isDragging) return;
    const interval = setInterval(updateProgress, 500);

    return () => {
      clearInterval(interval);
    };
  }, [updateProgress, isDragging]);

  const commitSeek = async (value: number) => {
    isDraggingRef.current = false;
    dragProgressRef.current = null;
    setProgress(value);
    setDragProgress(null);
    setIsDragging(false);
    await storeSeek(value);
  };

  useEffect(() => {
    if (!isDragging) return;

    const handleWindowPointerUp = () => {
      if (!isDraggingRef.current) return;
      const target = dragProgressRef.current ?? dragProgress;
      if (target !== null) {
        void commitSeek(target);
      } else {
        isDraggingRef.current = false;
        setIsDragging(false);
        setDragProgress(null);
      }
    };

    window.addEventListener("pointerup", handleWindowPointerUp);
    window.addEventListener("pointercancel", handleWindowPointerUp);
    return () => {
      window.removeEventListener("pointerup", handleWindowPointerUp);
      window.removeEventListener("pointercancel", handleWindowPointerUp);
    };
  }, [isDragging, dragProgress, storeSeek]);

  const handlePointerDown = (e: React.PointerEvent<HTMLInputElement>) => {
    isDraggingRef.current = true;
    setIsDragging(true);
    const initialProgress = sliderValueToProgress(Number(e.currentTarget.value));
    dragProgressRef.current = initialProgress;
    setDragProgress(initialProgress);
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = sliderValueToProgress(Number(e.target.value));
    if (isDraggingRef.current) {
      dragProgressRef.current = value;
      setDragProgress(value);
    } else {
      void commitSeek(value);
    }
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLInputElement>) => {
    if (!isDraggingRef.current) return;
    const value =
      dragProgressRef.current ?? sliderValueToProgress(Number(e.currentTarget.value));
    void commitSeek(value);
  };

  const handleMute = () => {
    setMutedVolume(volume);
    if (volume === 0) {
      return setVolume(mutedVolume);
    }
    setVolume(0);
  };

  const openModal = useModalStore((s) => s.openModal);
  const isCurrentFavorite = useFavoritesStore((s) =>
    currentSong ? s.favoriteIds.has(currentSong.id) : false,
  );
  const toggleFavorite = useFavoritesStore((s) => s.toggleFavorite);
  const albumUrl = streamAlbumUrl(currentSong?.source_url);
  const target = currentSong
    ? albumTarget(currentSong, isStreaming, albumUrl)
    : null;
  // A stream is not in the library: it can be downloaded (and only then added to a playlist).
  const streamTrack = useMemo(
    () =>
      currentSong && isStreaming
        ? streamSongToRemoteTrack(currentSong, albumUrl)
        : null,
    [currentSong, isStreaming, albumUrl],
  );
  const downloadState = useTrackStatus(
    streamTrack ? remoteTrackRef(streamTrack) : null,
  );
  const downloadParams = useMemo(
    () => (streamTrack ? remoteTrackDownloadParams(streamTrack) : null),
    [streamTrack],
  );
  // The download of this song started from anywhere (here, a menu, the album page), if any.
  const download = useLibraryStore((s) =>
    downloadParams
      ? findDownload(s.activeDownloads, downloadParams)
      : undefined,
  );
  const downloading = isRunning(download);
  const downloaded =
    downloadState === "downloaded" || download?.status === "completed";

  const openAlbum = () => {
    if (!currentSong || !target) return;
    if (target.kind === "library") {
      void useLibraryStore.getState().setSelectedTab(target.albumArtist);
      return;
    }
    const item: SearchSuggestion = {
      item_type: ItemType.Album,
      name: currentSong.album,
      band_name: currentSong.album_artist,
      item_url_path: target.url,
      img: currentSong.cover_url ?? "",
      platform: currentSong.source,
    };
    void useAlbumPreviewStore.getState().open(item);
  };

  const displayProgress = calculateDisplayProgress(progress, dragProgress);
  const currentTime = currentSong ? currentSong.duration_ms * displayProgress : 0;

  return (
    <div
      className={clsx(styles.container, { [styles.disabled]: !currentSong })}
      onDragStart={(e) => e.preventDefault()}
    >
      <input
        className={styles.progressBar}
        type="range"
        style={{
          background: `linear-gradient(to right, var(--color-primary) ${displayProgress * 100}%, #555 0)`,
        }}
        disabled={!currentSong || streamLoading}
        min={0}
        max={1000}
        value={progressToSliderValue(displayProgress)}
        onChange={handleChange}
        onPointerDown={handlePointerDown}
        onPointerUp={handlePointerUp}
      />
      <div className={styles.innerBlock}>
        <div className={styles.leftBlock}>
          <div className={styles.leftControls}>
            <button
              onClick={() => previous()}
              className={styles.leftControl}
              disabled={!currentSong}
            >
              <MdSkipPrevious />
            </button>
            <button
              className={styles.leftControl}
              onClick={togglePlay}
              disabled={!currentSong}
            >
              {streamLoading ? (
                <span
                  className={styles.loadingSpinner}
                  role="status"
                  aria-label="Loading stream"
                />
              ) : isPlaying ? (
                <MdPause />
              ) : (
                <MdPlayArrow />
              )}
            </button>
            <button
              onClick={() => next()}
              className={styles.leftControl}
              disabled={!currentSong}
            >
              <MdSkipNext />
            </button>
          </div>
          <div className={styles.progress}>
            {formatDuration(currentTime)} /{" "}
            {currentSong ? formatDuration(currentSong.duration_ms) : "0:00"}
          </div>
        </div>

        <div className={styles.currentSongBlock}>
          {currentSong ? (
            <>
              {currentSong.cover_url ? (
                <img
                  loading="lazy"
                  decoding="async"
                  className={styles.currentSongCover}
                  src={coverSrc(currentSong.cover_url, 64)}
                  alt={currentSong.title}
                />
              ) : (
                <img
                  className={styles.currentSongCover}
                  src={placeholderSongImage}
                  alt={currentSong.title}
                />
              )}
              <div className={styles.currentSongDetails}>
                <div className={clsx(styles.currentSongTitle, "truncate")}>
                  {currentSong.title}
                </div>
                {playbackError && (
                  <div
                    className={clsx(styles.playbackError, "truncate")}
                    title={playbackError.message}
                  >
                    Can't play this song: {playbackError.message}
                  </div>
                )}
                <div
                  className={clsx(
                    styles.currentSongArtist,
                    "truncate",
                    playbackError && styles.hidden,
                  )}
                >
                  {isStreaming && (
                    <span
                      className={styles.streamBadge}
                      title="Streaming from the internet, not saved to your library"
                    >
                      <MdCloudQueue size={14} />
                      Streaming
                    </span>
                  )}
                  {currentSong.album_artist}
                  {currentSong.album && " • "}
                  {target ? (
                    // A span, not a button: a button is one unbreakable box, so a long album name
                    // was cut away whole and left an empty gap instead of ending in "…".
                    <span
                      role="button"
                      tabIndex={0}
                      className={styles.albumLink}
                      onClick={openAlbum}
                      onKeyDown={(e) => {
                        if (e.key !== "Enter" && e.key !== " ") return;
                        e.preventDefault();
                        openAlbum();
                      }}
                      title="Open the album"
                    >
                      {currentSong.album}
                    </span>
                  ) : (
                    currentSong.album
                  )}
                  {currentSong.release_year
                    ? ` • ${currentSong.release_year}`
                    : ""}
                </div>
              </div>
              <div className={styles.songActions}>
                <button
                  type="button"
                  className={styles.songAction}
                  disabled={isStreaming}
                  title={
                    isStreaming
                      ? "Download the song first to add it to a playlist"
                      : "Add to playlist"
                  }
                  onClick={() =>
                    openModal(<AddToPlaylistModal songIds={[currentSong.id]} />)
                  }
                >
                  <MdPlaylistAdd />
                </button>
                <button
                  type="button"
                  className={clsx(styles.songAction, {
                    [styles.favoriteActive]: isCurrentFavorite,
                  })}
                  disabled={!currentSong}
                  title={
                    !currentSong
                      ? "No track playing"
                      : isCurrentFavorite
                        ? "Remove from favorites"
                        : "Add to favorites"
                  }
                  aria-label={
                    isCurrentFavorite
                      ? "Remove from favorites"
                      : "Add to favorites"
                  }
                  onClick={() => {
                    if (currentSong) {
                      void toggleFavorite(currentSong.id);
                    }
                  }}
                >
                  {isCurrentFavorite ? <MdFavorite /> : <MdFavoriteBorder />}
                </button>
                {streamTrack &&
                  (downloading || downloaded || !downloadParams ? (
                    <button
                      type="button"
                      className={styles.songAction}
                      disabled
                      title={
                        downloaded ? "Already in your library" : "Downloading…"
                      }
                    >
                      {downloading ? (
                        <span
                          className={styles.actionSpinner}
                          role="status"
                          aria-label="Downloading"
                        />
                      ) : (
                        <MdCheck />
                      )}
                    </button>
                  ) : (
                    <DownloadSplitButton
                      size={36}
                      revealGear
                      className={styles.downloadSplit}
                      icon={<MdFileDownload />}
                      title="Download this song"
                      onDownload={() =>
                        void useLibraryStore
                          .getState()
                          .startDownload(downloadParams)
                      }
                      onChooseQuality={() => openQualityMenu(downloadParams)}
                    />
                  ))}
              </div>
            </>
          ) : (
            ""
          )}
        </div>

        <div className={styles.rightControls}>
          <button
            onClick={cycleRepeatMode}
            className={clsx(styles.rightControl, {
              [styles.rightControlActive]: repeatMode !== RepeatMode.Off,
            })}
          >
            {repeatMode === RepeatMode.Track ? (
              <MdRepeatOne size={60} />
            ) : (
              <MdRepeat size={60} />
            )}
          </button>
          <button
            onClick={toggleShuffle}
            className={clsx(styles.rightControl, {
              [styles.rightControlActive]: shuffleMode,
            })}
          >
            <MdShuffle size={60} />
          </button>
          <button
            className={clsx(styles.rightControl, {
              [styles.rightControlActive]: volume > 0,
            })}
            onClick={handleMute}
          >
            {volume > 0 ? <MdVolumeUp size={60} /> : <MdVolumeOff size={60} />}
          </button>
          <input
            className={styles.volumeSlider}
            type="range"
            style={{
              background: `linear-gradient(to right, var(--color-primary) ${volume * 100}%, #555 0)`,
            }}
            min={0}
            max={100}
            value={volume * 100}
            onChange={(e) => setVolume(Number(e.target.value) / 100)}
          />
        </div>
      </div>
    </div>
  );
};
