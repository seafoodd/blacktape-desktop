import { useMemo, useState } from "react";
import styles from "@/layouts/left-sidebar.module.css";
import { AiOutlineSearch } from "react-icons/ai";
import { HiX } from "react-icons/hi";
import clsx from "clsx";
import { coverSrc } from "@/shared/lib/cover";
import placeholderArtistImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import Fuse from "fuse.js";
import {
  DownloadProgress,
  useLibraryStore,
} from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import CancelDownloadModal from "./CancelDownloadModal.tsx";

interface LibraryProps {
  className?: string;
}

interface DownloadingArtistGroup {
  artistName: string;
  coverUrl?: string;
  overallPercent: number;
  downloads: DownloadProgress[];
}

export const Library = ({ className }: LibraryProps) => {
  const { tabs, selectedTab, setSelectedTab, activeDownloads, cancelDownload } =
    useLibraryStore();
  const { unselectPlaylist } = usePlaylistStore();
  const { openModal } = useModalStore();
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

  const downloadingArtists = useMemo(() => {
    const activeList = Object.values(activeDownloads).filter(
      (d) =>
        d.status === "downloading" ||
        d.status === "idle" ||
        d.status === "processing",
    );

    const byArtist = new Map<string, DownloadingArtistGroup>();

    for (const d of activeList) {
      const key = d.artistName.trim().toLowerCase();
      const existing = byArtist.get(key);
      if (!existing) {
        byArtist.set(key, {
          artistName: d.artistName.trim(),
          coverUrl: d.coverUrl,
          overallPercent: d.overallPercent ?? 0,
          downloads: [d],
        });
      } else {
        existing.downloads.push(d);
        const sum = existing.downloads.reduce(
          (acc, curr) => acc + (curr.overallPercent ?? 0),
          0,
        );
        existing.overallPercent = Math.round(sum / existing.downloads.length);
        if (!existing.coverUrl && d.coverUrl) {
          existing.coverUrl = d.coverUrl;
        }
      }
    }

    let result = Array.from(byArtist.values());
    if (query.trim()) {
      const q = query.trim().toLowerCase();
      result = result.filter((a) => a.artistName.toLowerCase().includes(q));
    }
    return result;
  }, [activeDownloads, query]);

  const filteredTabs = useMemo(() => {
    let baseList = tabs;
    if (query.trim()) {
      baseList = fuse.search(query.trim()).map((result) => result.item);
    }
    const downloadingNames = new Set(
      downloadingArtists.map((d) => d.artistName.toLowerCase()),
    );
    return baseList.filter(
      (artist) => !downloadingNames.has(artist.name.toLowerCase()),
    );
  }, [fuse, tabs, query, downloadingArtists]);

  const handleCancelClick = (
    e: React.MouseEvent,
    artistGroup: DownloadingArtistGroup,
  ) => {
    e.stopPropagation();
    e.preventDefault();

    const first = artistGroup.downloads[0];
    const title =
      artistGroup.downloads.length === 1
        ? first.albumTitle || first.message || first.artistName
        : `${artistGroup.downloads.length} downloads for ${artistGroup.artistName}`;

    openModal(
      <CancelDownloadModal
        title={title}
        artist={artistGroup.artistName}
        onConfirm={() => {
          for (const d of artistGroup.downloads) {
            cancelDownload(d.id);
          }
        }}
      />,
    );
  };

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
        {/* Active Downloading Artists at the very top */}
        {downloadingArtists.map((artist) => (
          <button
            key={`downloading-${artist.artistName}`}
            className={clsx(styles.artistItem, {
              [styles.active]: selectedTab === artist.artistName,
            })}
            onClick={() => handleSelectTab(artist.artistName)}
          >
            <div className={styles.coverWrapper}>
              <img loading="lazy" decoding="async"
                src={
                  artist.coverUrl
                    ? artist.coverUrl.startsWith("http")
                      ? artist.coverUrl
                      : coverSrc(artist.coverUrl, 200)
                    : placeholderArtistImage
                }
                referrerPolicy="no-referrer"
                onError={(e) => {
                  if (e.currentTarget.src !== placeholderArtistImage) {
                    e.currentTarget.src = placeholderArtistImage;
                  }
                }}
                className={styles.artistImage}
                alt={artist.artistName}
              />
              <div
                className={styles.progressCircleOverlay}
                onClick={(e) => handleCancelClick(e, artist)}
                title="Cancel download"
              >
                <svg className={styles.progressSvg} viewBox="0 0 40 40">
                  <circle
                    className={styles.progressTrack}
                    cx="20"
                    cy="20"
                    r="16"
                    strokeWidth="3.5"
                    fill="transparent"
                  />
                  <circle
                    className={styles.progressBar}
                    cx="20"
                    cy="20"
                    r="16"
                    strokeWidth="3.5"
                    fill="transparent"
                    strokeDasharray={100.53}
                    strokeDashoffset={
                      100.53 *
                      (1 -
                        Math.min(100, Math.max(0, artist.overallPercent)) / 100)
                    }
                  />
                </svg>
                <span className={styles.progressPercentText}>
                  {artist.overallPercent}%
                </span>
                <HiX className={styles.progressCancelIcon} size={18} />
              </div>
            </div>
            <div className={styles.artistInfo}>
              <span className={clsx(styles.artistName, "truncate")}>
                {artist.artistName}
              </span>
              <span className={clsx(styles.downloadingStatusText, "truncate")}>
                Downloading... {artist.overallPercent}%
              </span>
            </div>
          </button>
        ))}

        {/* Existing Downloaded Artists */}
        {filteredTabs.map((artist) => (
          <button
            key={artist.name}
            className={clsx(styles.artistItem, {
              [styles.active]: selectedTab == artist.name,
            })}
            onClick={() => handleSelectTab(artist.name)}
          >
            <img loading="lazy" decoding="async"
              src={
                artist.cover_url
                  ? coverSrc(artist.cover_url, 64)
                  : placeholderArtistImage
              }
              referrerPolicy="no-referrer"
              onError={(e) => {
                if (e.currentTarget.src !== placeholderArtistImage) {
                  e.currentTarget.src = placeholderArtistImage;
                }
              }}
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
