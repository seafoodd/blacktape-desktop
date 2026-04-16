import { useCallback, useEffect, useState } from "react";
import styles from "./left-sidebar.module.css";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderArtistImage from "@/assets/react.svg";
import { ArtistSummary, getArtists } from "@/shared/lib/audio.ts";

const LeftSidebar = () => {
  const [artists, setArtists] = useState<ArtistSummary[]>([]);

  const [sidebarWidth, setSidebarWidth] = useState(260);
  const [isResizing, setIsResizing] = useState(false);

  const startResizing = useCallback(() => {
    setIsResizing(true);
  }, []);

  const stopResizing = useCallback(() => {
    setIsResizing(false);
  }, []);

  const resize = useCallback(
    (mouseMoveEvent: MouseEvent) => {
      if (isResizing) {
        // Limit the width between 150px and 600px
        const newWidth = Math.min(Math.max(150, mouseMoveEvent.clientX), 600);
        setSidebarWidth(newWidth);
      }
    },
    [isResizing],
  );

  useEffect(() => {
    window.addEventListener("mousemove", resize);
    window.addEventListener("mouseup", stopResizing);
    return () => {
      window.removeEventListener("mousemove", resize);
      window.removeEventListener("mouseup", stopResizing);
    };
  }, [resize, stopResizing]);

  useEffect(() => {
    getArtists().then(setArtists);
  }, []);
  return (
    <>
      <aside className={styles.leftSidebar} style={{ width: sidebarWidth }}>
        <h3 className={styles.leftSidebarTitle}>Artists</h3>
        <ul className={styles.artistList}>
          {artists.map((artist) => (
            <button key={artist.name} className={styles.artistItem}>
              {artist.cover_url ? (
                <img
                  src={convertFileSrc(artist.cover_url)}
                  className={styles.artistImage}
                  alt={artist.name}
                />
              ) : (
                <img
                  src={placeholderArtistImage}
                  className={styles.artistImage}
                  alt=""
                />
              )}
              <div className={styles.artistInfo}>
                <span className={styles.artistName}>{artist.name}</span>
                <span className={styles.albumCount}>
                  {artist.album_count} Albums
                </span>
              </div>
            </button>
          ))}
        </ul>
      </aside>

      {/* invisible handle that detects the drag */}
      <div className={styles.resizer} onMouseDown={startResizing} />
    </>
  );
};

export default LeftSidebar;
