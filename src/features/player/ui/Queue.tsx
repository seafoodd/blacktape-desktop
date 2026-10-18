import { useAudioStore } from "@/shared/store/audioStore.ts";
import { convertFileSrc } from "@tauri-apps/api/core";
import styles from "./queue.module.css";
import placeholderSongImage from "@/assets/react.svg";
import clsx from "clsx";
import { useEffect, useRef } from "react";
import { playSongInQueue } from "@/shared/lib/audio.ts";

// import { useLibraryStore } from "@/shared/store/libraryStore.ts";

interface QueueProps {
  className?: string;
}

const Queue = ({ className }: QueueProps) => {
  const { queue, currentSong } = useAudioStore();
  // const { albums } = useLibraryStore();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const activeItemRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    const activeItem = activeItemRef.current;

    if (container && activeItem) {
      const containerRect = container.getBoundingClientRect();
      const itemRect = activeItem.getBoundingClientRect();

      // Check if the item is fully inside the visible area
      const isVisible =
        itemRect.top >= containerRect.top &&
        itemRect.bottom <= containerRect.bottom;

      if (!isVisible) {
        activeItem.scrollIntoView({
          behavior: "smooth",
          block: "start",
        });
      }
    }
  }, [currentSong?.id]);

  const handlePlaySong = (index: number) => {
    playSongInQueue(index).catch((e) =>
      console.log("startPlayback error: ", e),
    );
  };

  if (queue.length < 1) {
    return (
      <div className={clsx(styles.container, className)}>Queue is empty.</div>
    );
  }

  return (
    <div ref={containerRef} className={clsx(styles.container, className)}>
      {queue.map((song, index) => {
        const isCurrent = song.id === currentSong?.id;

        return (
          <div
            key={`${song.id}-${index}`}
            ref={isCurrent ? activeItemRef : null}
            className={clsx(styles.item, {
              [styles.active]: isCurrent,
            })}
            onClick={() => handlePlaySong(index)}
          >
            <img
              className={styles.songCover}
              src={
                song.cover_url
                  ? convertFileSrc(song.cover_url)
                  : placeholderSongImage
              }
              alt={song.title}
            />
            <div className={styles.info}>
              <span className={styles.title}>{song.title}</span>
              <span className={styles.artist}>
                {Array.isArray(song.artists)
                  ? song.artists.join(", ")
                  : song.artists}
              </span>
            </div>
          </div>
        );
      })}
    </div>
  );
};

export default Queue;
