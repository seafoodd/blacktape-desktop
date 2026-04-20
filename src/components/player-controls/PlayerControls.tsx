import { useEffect, useState } from "react";
import { seek } from "@/shared/lib/audio.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import styles from "./player-controls.module.css";
import { formatDuration } from "@/shared/lib/time.ts";
import {
  MdPause,
  MdPlayArrow,
  MdRepeat,
  MdShuffle,
  MdSkipNext,
  MdSkipPrevious,
  MdVolumeOff,
  MdVolumeUp,
} from "react-icons/md";
import clsx from "clsx";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderSongImage from "@/assets/react.svg";

const PlayerControls = () => {
  const {
    currentSong,
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
          <button onClick={() => previous()} className={styles.leftControl}>
            <MdSkipPrevious />
          </button>
          <button className={styles.leftControl} onClick={togglePlay}>
            {isPlaying ? <MdPause /> : <MdPlayArrow />}
          </button>
          <button onClick={() => next()} className={styles.leftControl}>
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
                  className={styles.currentSongCover}
                  src={convertFileSrc(currentSong.cover_url)}
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
                <div className={clsx(styles.currentSongArtist, "truncate")}>
                  {[
                    currentSong.artist,
                    currentSong.album,
                    currentSong.release_year,
                  ]
                    .filter(Boolean)
                    .join(" • ")}
                </div>
              </div>
            </>
          ) : (
            ""
          )}
        </div>

        <div className={styles.rightControls}>
          <button className={styles.rightControl}>
            <MdRepeat size={60} />
          </button>
          <button className={styles.rightControl}>
            <MdShuffle size={60} />
          </button>
          <button className={styles.rightControl} onClick={handleMute}>
            {volume > 0 ? <MdVolumeUp size={60} /> : <MdVolumeOff size={60} />}
          </button>
          <input
            className={styles.volumeSlider}
            type="range"
            style={{
              background: `linear-gradient(to right, var(--color-primary-dark) ${volume * 100}%, #555 0)`,
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

export default PlayerControls;
