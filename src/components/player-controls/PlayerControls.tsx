import { useEffect, useState } from "react";
import { seek } from "../../shared/lib/audio";
import { useAudioStore } from "../../shared/store/audioStore";
import styles from "./player-controls.module.css";
import { formatDuration } from "../../shared/lib/time";
import { FaPlay } from "react-icons/fa6";
import { FaPause } from "react-icons/fa6";
import {
  TbPlayerSkipBackFilled,
  TbPlayerSkipForwardFilled,
} from "react-icons/tb";

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

  const currentTime = currentSong
    ? {
        secs: Math.floor(currentSong.duration.secs * progress),
        nanos: Math.floor(currentSong.duration.nanos * progress),
      }
    : { secs: 0, nanos: 0 };

  return (
    <div
      className={`${styles.container} ${currentSong ? "" : styles.disabled}`}
      onDragStart={(e) => e.preventDefault()}
    >
      <div className={styles.leftControls}>
        <button>
          <TbPlayerSkipBackFilled size={24} />
        </button>
        <button onClick={togglePlay}>
          {isPlaying ? <FaPause size={25} /> : <FaPlay size={24} />}
        </button>
        <button>
          <TbPlayerSkipForwardFilled size={24} />
        </button>
      </div>
      <div className={styles.progress}>
        {formatDuration(currentTime)} /{" "}
        {currentSong ? formatDuration(currentSong.duration) : "0:00"}
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
        <button className={styles.rightControl}></button>
        <button className={styles.rightControl}></button>
        <button className={styles.rightControl}></button>
      </div>
    </div>
  );
};

export default PlayerControls;
