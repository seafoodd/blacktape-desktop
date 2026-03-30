import { useEffect, useState } from "react";
import { seek } from "../../shared/lib/audio";
import { useAudioStore } from "../../shared/store/audioStore";
import styles from "./player-controls.module.css";

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
    const interval = setInterval(updateProgress, 1000);

    return () => {
      clearInterval(interval);
    };
  }, [updateProgress, isDragging]);

  const handleSeek = async (value: number) => {
    setProgress(value);
    if (isDragging) return;
    await storeSeek(value);
  };

  if (!currentSong) return null;

  return (
    <div className="">
      <div className="player-controls" onDragStart={(e) => e.preventDefault()}>
        <button className={styles["play-button"]} onClick={togglePlay}>
          {isPlaying ? "Pause" : "Play"}
        </button>

        <input
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
          style={{ width: 300 }}
        />
        {progress}
      </div>
    </div>
  );
};

export default PlayerControls;
