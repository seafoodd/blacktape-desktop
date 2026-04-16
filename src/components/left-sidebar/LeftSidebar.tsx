import { useCallback, useEffect, useState } from "react";
import styles from "./left-sidebar.module.css";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderArtistImage from "@/assets/react.svg";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import clsx from "clsx";

const LeftSidebar = () => {
  const { fetchTabs, tabs, selectedTab, setSelectedTab } = useLibraryStore();
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
    fetchTabs().catch((e) => {
      console.log("fetch tabs error: ", e);
    });
  }, []);
  return (
    <>
      <aside className={styles.leftSidebar} style={{ width: sidebarWidth }}>
        <h3 className={styles.leftSidebarTitle}>Artists</h3>
        <ul className={styles.artistList}>
          {tabs.map((artist) => (
            <button
              key={artist.name}
              className={clsx(styles.artistItem, {
                [styles.active]: selectedTab == artist.name,
              })}
              onClick={() => setSelectedTab(artist.name)}
            >
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
                  alt={artist.name}
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
