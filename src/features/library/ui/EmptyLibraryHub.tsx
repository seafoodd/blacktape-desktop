import React, { useState } from "react";
import { AiOutlineArrowRight, AiOutlineSearch } from "react-icons/ai";
import { MdExplore, MdFolder, MdFolderOpen, MdGraphicEq } from "react-icons/md";
import { pickFolder } from "@/shared/lib/ui/dialog.ts";
import { scanMusic } from "@/shared/lib/audio.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useSearchPageStore } from "@/features/search/model/searchPageStore.ts";
import { useScanStore } from "@/features/library/model/scanStore.ts";
import {
  INSPIRATION_TAGS,
  cleanSearchQuery,
  formatFolderScanningLabel,
} from "./emptyLibraryHubHelper.ts";
import styles from "./empty-library-hub.module.css";

export const EmptyLibraryHub: React.FC = () => {
  const [searchQuery, setSearchQuery] = useState("");
  const [isLocalPicking, setIsLocalPicking] = useState(false);
  const scanProgress = useScanStore((s) => s.progress);
  const isScanning = scanProgress.phase === "scanning" || isLocalPicking;

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const q = cleanSearchQuery(searchQuery);
    if (!q) return;
    useSearchPageStore.getState().open(q);
  };

  const handleTagClick = (tag: string) => {
    useSearchPageStore.getState().open(tag);
  };

  const handleChooseFolder = async () => {
    setIsLocalPicking(true);
    try {
      const dir = await pickFolder();
      if (!dir) {
        setIsLocalPicking(false);
        return;
      }
      localStorage.setItem("library_dir", dir);
      await scanMusic(dir);
      await useLibraryStore.getState().fetchTabs();
    } catch (err) {
      console.error("Failed to select or scan music directory:", err);
    } finally {
      setIsLocalPicking(false);
    }
  };

  const folderButtonText = formatFolderScanningLabel(
    isScanning,
    scanProgress.phase === "scanning" ? scanProgress.scanned : undefined,
  );

  return (
    <div className={styles.container} data-testid="empty-library-hub">
      <div className={styles.hero}>
        <div className={styles.iconWrapper}>
          <MdGraphicEq size={28} />
        </div>
        <h1 className={styles.title}>Welcome to Blacktape</h1>
        <p className={styles.subtitle}>
          Your local and streaming music player. Stream and download albums online,
          or import your local music collection to get started.
        </p>
      </div>

      <div className={styles.cardsGrid}>
        {/* Card 1: Online Search & Stream */}
        <div className={styles.card}>
          <div className={styles.cardHeader}>
            <div className={styles.cardIconExplore}>
              <MdExplore size={22} />
            </div>
            <span className={styles.badgeExplore}>Streaming & Downloads</span>
          </div>

          <h2 className={styles.cardTitle}>Discover Music Online</h2>
          <p className={styles.cardDesc}>
            Search millions of tracks, albums, and artists. Stream immediately
            or download high-quality releases directly to your offline library.
          </p>

          <form className={styles.searchForm} onSubmit={handleSearchSubmit}>
            <div className={styles.searchInputWrapper}>
              <AiOutlineSearch size={16} className={styles.searchIcon} />
              <input
                type="text"
                className={styles.searchInput}
                placeholder="Search artists, tracks, or albums..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
            <button
              type="submit"
              className={styles.searchButton}
              disabled={!cleanSearchQuery(searchQuery)}
              title="Search online music"
            >
              <span>Search</span>
              <AiOutlineArrowRight size={14} />
            </button>
          </form>

          <div className={styles.tagsWrapper}>
            <span className={styles.tagsLabel}>Popular searches</span>
            <div className={styles.tagsList}>
              {INSPIRATION_TAGS.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  className={styles.tagPill}
                  onClick={() => handleTagClick(tag)}
                >
                  {tag}
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Card 2: Local Folder Import */}
        <div className={styles.card}>
          <div className={styles.cardHeader}>
            <div className={styles.cardIconFolder}>
              <MdFolder size={22} />
            </div>
            <span className={styles.badgeFolder}>Local Audio Library</span>
          </div>

          <h2 className={styles.cardTitle}>Add Local Music</h2>
          <p className={styles.cardDesc}>
            Select a folder on your computer where your music is stored.
            Blacktape will automatically scan your files, organize albums, and load cover art.
          </p>

          <div className={styles.folderActionWrapper}>
            <button
              type="button"
              className={styles.folderButton}
              onClick={handleChooseFolder}
              disabled={isScanning}
            >
              {isScanning ? (
                <>
                  <MdFolderOpen size={18} />
                  <span>{folderButtonText}</span>
                </>
              ) : (
                <>
                  <MdFolder size={18} />
                  <span>{folderButtonText}</span>
                </>
              )}
            </button>

            <span className={styles.formatsNotice}>
              Supported formats: MP3, FLAC, M4A, OGG, WAV, OPUS
            </span>
          </div>
        </div>
      </div>

      <div className={styles.footerTips}>
        <div className={styles.tipItem}>
          <kbd className={styles.kbd}>Ctrl</kbd> + <kbd className={styles.kbd}>K</kbd> to search
        </div>
        <div className={styles.tipItem}>
          <kbd className={styles.kbd}>Space</kbd> to play / pause
        </div>
        <div className={styles.tipItem}>
          <kbd className={styles.kbd}>Esc</kbd> to dismiss dialogs
        </div>
      </div>
    </div>
  );
};

