import React, { useEffect, useMemo, useState } from "react";
import styles from "./search-bar.module.css";
import { AiOutlineArrowRight, AiOutlineSearch } from "react-icons/ai";
import { BiDownload, BiFilterAlt, BiInfoCircle } from "react-icons/bi";
import clsx from "clsx";
import { formatCompactNumber } from "@/shared/lib/number.ts";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { FaCheckCircle } from "react-icons/fa";

const formatSuggestionType = (type: ItemType): string => {
  switch (type) {
    case ItemType.Album:
      return "ALBUM";
    case ItemType.Artist:
      return "ARTIST";
    case ItemType.Track:
      return "TRACK";
    default:
      return "UNKNOWN";
  }
};

type Platform = "All" | "Youtube" | "Bandcamp" | "Local";

interface SearchBarProps {
  className?: string;
}

export const SearchBar = ({ className }: SearchBarProps) => {
  const [query, setQuery] = useState<string>("");
  const [debouncedQuery, setDebouncedQuery] = useState<string>(query);
  const [isSearching, setIsSearching] = useState<boolean>(false);
  const [isFilterMenuOpen, setIsFilterMenuOpen] = useState<boolean>(false);
  const [showDropdown, setShowDropdown] = useState<boolean>(false);

  const {
    searchResults,
    setSearchQuery,
    executeSearch,
    commitSearch,
    activeCategory,
    activePlatform,
    setActiveCategory,
    setActivePlatform,
    startDownload,
    activeDownloads,
    initDownloadListeners,
  } = useLibraryStore();

  useEffect(() => {
    initDownloadListeners().catch(console.error);
  }, [initDownloadListeners]);

  const handleBrowseAll = (e: React.MouseEvent) => {
    e.preventDefault();
    commitSearch(query);
    setShowDropdown(false);
  };

  useEffect(() => {
    if (query.trim() !== "") {
      setIsSearching(true);
      setShowDropdown(true);
    } else {
      setShowDropdown(false);
    }
    const handler = setTimeout(() => {
      setDebouncedQuery(query);
    }, 200);

    return () => clearTimeout(handler);
  }, [query]);

  useEffect(() => {
    setSearchQuery(debouncedQuery);
    if (debouncedQuery.trim() === "") {
      setIsSearching(false);
      return;
    }

    executeSearch(debouncedQuery).finally(() => {
      setIsSearching(false);
    });
  }, [debouncedQuery, setSearchQuery, executeSearch]);

  const filteredResults = useMemo(() => {
    return searchResults.filter((item) => {
      const matchesCategory =
        activeCategory === "All" ||
        item.item_type.toLowerCase() === activeCategory.toLowerCase();
      const matchesPlatform =
        activePlatform === "All" ||
        item.platform?.toLowerCase() === activePlatform.toLowerCase();
      return matchesCategory && matchesPlatform;
    });
  }, [searchResults, activeCategory, activePlatform]);

  return (
    <div className={clsx(styles.container, className)}>
      <div className={styles.searchFieldWrapper}>
        <AiOutlineSearch size={18} className={styles.searchIcon} />

        <input
          className={styles.input}
          type="text"
          placeholder="Search music..."
          value={query}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim() !== "") {
              commitSearch(query);
              setShowDropdown(false);
              e.currentTarget.blur();
            }
          }}
        />

        <button
          type="button"
          className={clsx(
            styles.filterToggleBtn,
            (activeCategory !== "All" || activePlatform !== "All") &&
              styles.filterActive,
          )}
          onClick={() => setIsFilterMenuOpen(!isFilterMenuOpen)}
          onBlur={() => setIsFilterMenuOpen(false)}
        >
          <BiFilterAlt size={18} />
        </button>

        {isFilterMenuOpen && (
          <div
            className={styles.filterPopover}
            onMouseDown={(e) => e.preventDefault()}
          >
            <div className={styles.filterSection}>
              <span className={styles.sectionLabel}>Platforms</span>
              <div className={styles.pillGrid}>
                {(["All", "Youtube", "Bandcamp", "Local"] as Platform[]).map(
                  (plat) => (
                    <button
                      key={plat}
                      type="button"
                      className={clsx(
                        styles.filterPill,
                        activePlatform === plat && styles.pillActive,
                      )}
                      onClick={() => setActivePlatform(plat)}
                    >
                      {plat.toLowerCase()}
                    </button>
                  ),
                )}
              </div>
            </div>

            <div className={styles.filterSection}>
              <span className={styles.sectionLabel}>Categories</span>
              <div className={styles.pillGrid}>
                <button
                  type="button"
                  className={clsx(
                    styles.filterPill,
                    activeCategory === "All" && styles.pillActive,
                  )}
                  onClick={() => setActiveCategory("All")}
                >
                  all items
                </button>
                <button
                  type="button"
                  className={clsx(
                    styles.filterPill,
                    activeCategory === ItemType.Track && styles.pillActive,
                  )}
                  onClick={() => setActiveCategory(ItemType.Track)}
                >
                  tracks
                </button>
                <button
                  type="button"
                  className={clsx(
                    styles.filterPill,
                    activeCategory === ItemType.Album && styles.pillActive,
                  )}
                  onClick={() => setActiveCategory(ItemType.Album)}
                >
                  albums
                </button>
                <button
                  type="button"
                  className={clsx(
                    styles.filterPill,
                    activeCategory === ItemType.Artist && styles.pillActive,
                  )}
                  onClick={() => setActiveCategory(ItemType.Artist)}
                >
                  artists
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      {showDropdown && (
        <div className={styles.suggestions}>
          <div className={styles.suggestionsList}>
            {isSearching ? (
              Array.from({ length: 4 }).map((_, i) => (
                <div key={i} className={styles.skeletonItem}>
                  <div className={styles.skeletonCover} />
                  <div className={styles.skeletonRightBlock}>
                    <div className={styles.skeletonLineTitle} />
                    <div className={styles.skeletonLineMeta} />
                  </div>
                </div>
              ))
            ) : filteredResults.length === 0 ? (
              <div className={styles.noResults}>
                <BiInfoCircle size={24} />
                <p>No results found matching your active filter criteria</p>
              </div>
            ) : (
              filteredResults.slice(0, 20).map((suggestion, index) => {
                const activeDownload = Object.values(activeDownloads).find(
                  (d) => d.url === suggestion.item_url_path,
                );

                return (
                  <div key={index} className={styles.suggestion}>
                    <img
                      className={clsx(styles.cover, {
                        [styles.artist]:
                          suggestion.item_type === ItemType.Artist,
                      })}
                      src={suggestion.img}
                      referrerPolicy="no-referrer"
                      alt="cover"
                    />
                    <div className={clsx(styles.rightBlock, "truncate")}>
                      <p className={clsx(styles.title, "truncate")}>
                        {suggestion.name}
                      </p>
                      <p className={clsx(styles.artist, "truncate")}>
                        {suggestion.item_type === ItemType.Artist ? (
                          <span className={styles.followers}>
                            {suggestion.subscriber_count ? (
                              <span className={"uppercase"}>
                                {formatCompactNumber(
                                  suggestion.subscriber_count,
                                )}{" "}
                                subscribers
                              </span>
                            ) : (
                              ""
                            )}
                          </span>
                        ) : (
                          `by ${suggestion.band_name}`
                        )}
                      </p>
                      <div className={styles.metaRow}>
                        <span className={styles.type}>
                          {formatSuggestionType(suggestion.item_type)}
                        </span>
                        {suggestion.platform && (
                          <span className={styles.badge}>
                            {suggestion.platform}
                          </span>
                        )}
                      </div>
                    </div>

                    <div className={styles.tools}>
                      {suggestion.item_type !== ItemType.Artist && (
                        <>
                          {!activeDownload ||
                          activeDownload.status === "failed" ? (
                            <button
                              className={styles.downloadButton}
                              onMouseDown={(e) => e.preventDefault()}
                              onClick={() => {
                                startDownload(
                                  suggestion.platform,
                                  suggestion.item_type,
                                  suggestion.item_url_path,
                                ).catch((e) => {
                                  console.log("Download error: ", e);
                                });
                              }}
                            >
                              <BiDownload size={20} />
                            </button>
                          ) : (
                            <div className={styles.downloadStatus}>
                              {activeDownload.status === "completed" && (
                                <FaCheckCircle size={20} />
                              )}
                              {(activeDownload.status === "idle" ||
                                activeDownload.status === "downloading" ||
                                activeDownload.status === "processing") && (
                                <span>
                                  {activeDownload.current > 0
                                    ? Math.round(
                                        (activeDownload.current /
                                          activeDownload.total) *
                                          100,
                                      )
                                    : 0}
                                  %
                                </span>
                              )}
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                );
              })
            )}
          </div>

          {!isSearching && filteredResults.length > 0 && (
            <a
              className={styles.browseButton}
              href="#"
              onClick={handleBrowseAll}
              onMouseDown={(e) => e.preventDefault()}
            >
              Browse all results <AiOutlineArrowRight size={20} />
            </a>
          )}
        </div>
      )}
    </div>
  );
};