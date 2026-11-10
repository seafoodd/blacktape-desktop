import { useEffect, useMemo, useState } from "react";
import { seek } from "@/shared/lib/audio.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import styles from "./player-controls.module.css";
import { formatDuration } from "@/shared/lib/time.ts";
import {
  MdCloudQueue,
  MdCheck,
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
import { coverSrc } from "@/shared/lib/cover";
import placeholderSongImage from "@/assets/react.svg";
import { ItemType, useLibraryStore, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { remoteTrackDownloadParams } from "@/features/remote-track-menu/model/buildRemoteTrackMenu.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { useTrackStatus } from "@/shared/hooks/useDownloadStatus.ts";
import { findDownload, isRunning } from "@/shared/lib/downloadMatch.ts";
import { streamAlbumUrl } from "@/shared/lib/stream.ts";
import { albumTarget, remoteTrackRef, streamSongToRemoteTrack } from "../lib/currentSong.ts";
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
  const [mutedVolume, setMutedVolume] = useState<number>(volume);

  useEffect(() => {
    if (isDragging) return;
    const interval = setInterval(updateProgress, 500);

    return () => {
      clearInterval(interval);
    };
  }, [updateProgress, isDragging]);

  const handleSeek = async (value: number) => {
    setProgress(value);
    if (isDragging) return;
    await storeSeek(value);
  };

  const handleMute = () => {
    setMutedVolume(volume);
    if (volume === 0) {
      return setVolume(mutedVolume);
    }
    setVolume(0);
  };

  const openModal = useModalStore((s) => s.openModal);
  const albumUrl = streamAlbumUrl(currentSong?.source_url);
  const target = currentSong ? albumTarget(currentSong, isStreaming, albumUrl) : null;
  // A stream is not in the library: it can be downloaded (and only then added to a playlist).
  const streamTrack = useMemo(
    () => (currentSong && isStreaming ? streamSongToRemoteTrack(currentSong, albumUrl) : null),
    [currentSong, isStreaming, albumUrl],
  );
  const downloadState = useTrackStatus(streamTrack ? remoteTrackRef(streamTrack) : null);
  const downloadParams = useMemo(() => (streamTrack ? remoteTrackDownloadParams(streamTrack) : null), [streamTrack]);
  // The download of this song started from anywhere (here, a menu, the album page), if any.
  const download = useLibraryStore((s) => (downloadParams ? findDownload(s.activeDownloads, downloadParams) : undefined));
  const downloading = isRunning(download);
  const downloaded = downloadState === "downloaded" || download?.status === "completed";

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

  const currentTime = currentSong ? currentSong.duration_ms * progress : 0;

  return (
    <div
      className={clsx(styles.container, { [styles.disabled]: !currentSong })}
      onDragStart={(e) => e.preventDefault()}
    >
      <input
        className={styles.progressBar}
        type="range"
        style={{
          background: `linear-gradient(to right, var(--color-primary) ${progress * 100}%, #555 0)`,
        }}
        disabled={!currentSong || streamLoading}
        min={0}
        max={1000}
        value={progress * 1000}
        onChange={(e) => handleSeek(Number(e.target.value) / 1000)}
        onMouseDown={() => {
          setIsDragging(true);
        }}
        onTouchStart={() => {
          setIsDragging(true);
        }}
        onMouseUp={async (e) => {
          setIsDragging(false);
          await seek(Number(e.currentTarget.value) / 1000);
        }}
        onTouchEnd={async (e) => {
          setIsDragging(false);
          await seek(Number(e.currentTarget.value) / 1000);
        }}
      />
      <div className={styles.innerBlock}>
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
                  <div className={clsx(styles.playbackError, "truncate")} title={playbackError.message}>
                    Can't play this song: {playbackError.message}
                  </div>
                )}
                <div className={clsx(styles.currentSongArtist, "truncate", playbackError && styles.hidden)}>
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
                    <button
                      type="button"
                      className={styles.albumLink}
                      onClick={openAlbum}
                      title="Open the album"
                    >
                      {currentSong.album}
                    </button>
                  ) : (
                    currentSong.album
                  )}
                  {currentSong.release_year ? ` • ${currentSong.release_year}` : ""}
                </div>
              </div>
              <div className={styles.songActions}>
                {streamTrack && (
                  <button
                    type="button"
                    className={styles.songAction}
                    disabled={downloaded || downloading}
                    title={downloaded ? "Already in your library" : downloading ? "Downloading…" : "Download this song"}
                    onClick={() => downloadParams && void useLibraryStore.getState().startDownload(downloadParams)}
                  >
                    {downloading ? (
                      <span className={styles.actionSpinner} role="status" aria-label="Downloading" />
                    ) : downloaded ? (
                      <MdCheck />
                    ) : (
                      <MdFileDownload />
                    )}
                  </button>
                )}
                <button
                  type="button"
                  className={styles.songAction}
                  disabled={isStreaming}
                  title={isStreaming ? "Download the song first to add it to a playlist" : "Add to playlist"}
                  onClick={() => openModal(<AddToPlaylistModal songIds={[currentSong.id]} />)}
                >
                  <MdPlaylistAdd />
                </button>
                <button
                  type="button"
                  className={styles.songAction}
                  disabled
                  title="Favorites are not implemented yet"
                >
                  <MdFavoriteBorder />
                </button>
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