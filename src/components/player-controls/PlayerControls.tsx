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
  MdVolumeUp,
} from "react-icons/md";

const PlayerControls = () => {
  const {
    currentSong,
    progress,
    isPlaying,
    togglePlay,
    setProgress,
    seek: storeSeek,
    updateProgress,
  } = useAudioStore();
  const [isDragging, setIsDragging] = useState<boolean>(false);

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

  const currentTime = currentSong ? currentSong.duration_ms * progress : 0;

  return (
    <div
      className={`${styles.container} ${currentSong ? "" : styles.disabled}`}
      onDragStart={(e) => e.preventDefault()}
    >
      <div className={styles.leftControls}>
        <button className={styles.leftControl}>
          <MdSkipPrevious />
        </button>
        <button className={styles.leftControl} onClick={togglePlay}>
          {isPlaying ? <MdPause /> : <MdPlayArrow />}
        </button>
        <button className={styles.leftControl}>
          <MdSkipNext />
        </button>
      </div>
      <div className={styles.progress}>
        {formatDuration(currentTime)} /{" "}
        {currentSong ? formatDuration(currentSong.duration_ms) : "0:00"}
      </div>

      <input
        className={styles.progressBar}
        type="range"
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
          seek(Number(e.currentTarget.value) / 1000);
        }}
        onTouchEnd={async (e) => {
          setIsDragging(false);
          seek(Number(e.currentTarget.value) / 1000);
        }}
      />
      <div className={styles.rightControls}>
        <button className={styles.rightControl}>
          <MdVolumeUp size={60} />
        </button>
        <button className={styles.rightControl}>
          <MdRepeat size={60} />
        </button>
        <button className={styles.rightControl}>
          <MdShuffle size={60} />
        </button>
      </div>
    </div>
  );
};

export default PlayerControls;
