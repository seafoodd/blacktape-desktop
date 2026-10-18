import { useMemo, useState } from "react";
import styles from "@/layouts/left-sidebar.module.css";
import { AiOutlineSearch } from "react-icons/ai";
import clsx from "clsx";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderArtistImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import Fuse from "fuse.js";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";

interface LibraryProps {
  className?: string;
}

export const Library = ({ className }: LibraryProps) => {
  const { tabs, selectedTab, setSelectedTab } = useLibraryStore();
  const { unselectPlaylist } = usePlaylistStore();
  const [query, setQuery] = useState("");

  const fuse = useMemo(() => {
    return new Fuse(tabs, {
      keys: ["name"],
      threshold: 0.4,
      ignoreLocation: true,
    });
  }, [tabs]);

  const handleSelectTab = async (artistName: string) => {
    setSelectedTab(artistName);
    unselectPlaylist();
  };

  const filteredTabs = useMemo(() => {
    if (!query.trim()) return tabs;
    return fuse.search(query.trim()).map((result) => result.item);
  }, [fuse, tabs, query]);

  return (
    <div className={clsx(styles.activeTabWrapper, className)}>
      <div className={styles.searchWrapper}>
        <input
          className={styles.searchInput}
          type="text"
          placeholder="Search artists..."
          value={query}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim() !== "") {
              e.preventDefault();
              const firstItem = document.querySelector<HTMLElement>(
                `.${styles.artistItem}`,
              );
              firstItem?.focus();
            }
          }}
          onFocus={(e) => e.currentTarget.select()}
        />
        <AiOutlineSearch size={18} className={styles.searchIcon} />
      </div>

      <ul className={styles.artistList}>
        {filteredTabs.map((artist) => (
          <button
            key={artist.name}
            className={clsx(styles.artistItem, {
              [styles.active]: selectedTab == artist.name,
            })}
            onClick={() => handleSelectTab(artist.name)}
          >
            <img
              src={
                artist.cover_url
                  ? convertFileSrc(artist.cover_url)
                  : placeholderArtistImage
              }
              className={styles.artistImage}
              alt={artist.name}
            />
            <div className={styles.artistInfo}>
              <span className={clsx(styles.artistName, "truncate")}>
                {artist.name}
              </span>
              <span className={clsx(styles.albumCount, "truncate")}>
                {artist.album_count} Album{artist.album_count === 1 ? "" : "s"}
              </span>
            </div>
          </button>
        ))}
      </ul>
    </div>
  );
};
