import { coverSrc } from "@/shared/lib/cover";
import { useEffect, useMemo, useState } from "react";
import styles from "./search-bar.module.css";
import { AiOutlineArrowRight, AiOutlineSearch } from "react-icons/ai";
import { BiDownload, BiFilterAlt, BiInfoCircle } from "react-icons/bi";
import clsx from "clsx";
import { formatCompactNumber } from "@/shared/lib/number.ts";
import {
  ItemType,
  PlatformFilter,
  type SearchSuggestion,
  useLibraryStore,
} from "@/shared/store/libraryStore.ts";
import { FaCheckCircle } from "react-icons/fa";
import type { ReactNode } from "react";
import { flyToLibrary } from "@/shared/lib/flyAnimation.ts";
import {
  matchesPlatformFilter,
  PLATFORM_FILTERS,
} from "@/shared/lib/platforms.ts";
import {
  remoteRefOf,
  useAlbumStatus,
  useTrackStatus,
} from "@/shared/hooks/useDownloadStatus.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { useSearchPageStore } from "../model/searchPageStore.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { searchItemDownloadParams, trackAlbumItem } from "@/pages/search-results/searchResultsFormat.ts";

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

/** Tells whether a suggestion is already in the library (tracks by the library index, albums by their source). */
const InLibrary = ({
  item,
  children,
}: {
  item: SearchSuggestion;
  children: (inLibrary: boolean) => ReactNode;
}) => {
  const ref = remoteRefOf(item);
  const isAlbum = item.item_type === ItemType.Album;
  const trackState = useTrackStatus(isAlbum ? null : ref);
  const albumState = useAlbumStatus(isAlbum ? ref : null).state;
  return <>{children((isAlbum ? albumState : trackState) === "downloaded")}</>;
};

/** Library badge of a suggestion row: tracks are looked up in the library index, albums in the album sources. */
const SuggestionStatus = ({ item }: { item: SearchSuggestion }) => {
  const ref = remoteRefOf(item);
  const isAlbum = item.item_type === ItemType.Album;
  const trackState = useTrackStatus(isAlbum ? null : ref);
  const albumState = useAlbumStatus(isAlbum ? ref : null).state;
  // "Downloaded" is shown in place of the download button; only a partial album gets the badge here.
  const state = isAlbum ? albumState : trackState;
  return <DownloadStatusBadge state={state === "partial" ? "partial" : "none"} />;
};

/** The album a suggestion opens in the album preview: the album itself, or the album of a track (when known). */
const previewTarget = (item: SearchSuggestion): SearchSuggestion | null => {
  if (item.item_type === ItemType.Album) return item;
  if (item.item_type === ItemType.Track) return trackAlbumItem(item);
  return null;
};

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

  const openSearchPage = useSearchPageStore((s) => s.open);

  const handleBrowseAll = (e: React.MouseEvent) => {
    e.preventDefault();
    openSearchPage(query);
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
      return (
        matchesCategory && matchesPlatformFilter(item.platform, activePlatform)
      );
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
              openSearchPage(query);
              setShowDropdown(false);
              e.currentTarget.blur();
            }
          }}
          onFocus={(e) => e.currentTarget.select()}
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
                {PLATFORM_FILTERS.map(
                  (plat: PlatformFilter) => (
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

                const target = previewTarget(suggestion);

                return (
                  <div
                    key={index}
                    className={clsx(styles.suggestion, target && styles.suggestionClickable)}
                    title={target ? `Open album ${target.name}` : undefined}
                    onMouseDown={target ? (e) => e.preventDefault() : undefined}
                    onClick={
                      target
                        ? () => {
                            setShowDropdown(false);
                            void useAlbumPreviewStore.getState().open(target);
                          }
                        : undefined
                    }
                  >
                    <img
                      className={clsx(styles.cover, {
                        [styles.artist]:
                          suggestion.item_type === ItemType.Artist,
                      })}
                      src={coverSrc(suggestion.img, 56)}
                      referrerPolicy="no-referrer"
                      alt="cover"
                    />
                    <div className={clsx(styles.rightBlock, "truncate")}>
                      <div className={styles.titleRow}>
                        <p className={clsx(styles.title, "truncate")}>
                          {suggestion.name}
                        </p>
                        {suggestion.item_type !== ItemType.Artist && (
                          <SuggestionStatus item={suggestion} />
                        )}
                      </div>
                      <p className={clsx(styles.artist, "truncate")}>
                        {suggestion.item_type === ItemType.Artist ? (
                          <span className={styles.followers}>
                            {suggestion.subscriber_count &&
                            suggestion.audience_kind ? (
                              <span className={"uppercase"}>
                                {formatCompactNumber(
                                  suggestion.subscriber_count,
                                )}{" "}
                                {suggestion.audience_kind === "monthly_audience"
                                  ? "monthly audience"
                                  : "subscribers"}
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

                    <div className={styles.tools} onClick={(e) => e.stopPropagation()}>
                      {suggestion.item_type !== ItemType.Artist && (
                        <>
                          <InLibrary item={suggestion}>
                            {(inLibrary) =>
                              inLibrary &&
                              (!activeDownload || activeDownload.status === "failed") ? (
                                <div className={styles.downloadStatus} title="Already in your library">
                                  <FaCheckCircle size={20} />
                                </div>
                              ) : (
                                <>
                          {!activeDownload ||
                          activeDownload.status === "failed" ? (
                            <button
                              className={styles.downloadButton}
                              onMouseDown={(e) => e.preventDefault()}
                              onClick={(e) => {
                                const rect =
                                  e.currentTarget.getBoundingClientRect();
                                flyToLibrary(rect, suggestion.img);
                                startDownload(searchItemDownloadParams(suggestion)).catch((err) => {
                                  console.log("Download error: ", err);
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
                                  {activeDownload.overallPercent !== undefined
                                    ? activeDownload.overallPercent
                                    : activeDownload.current > 0
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
                              )
                            }
                          </InLibrary>
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