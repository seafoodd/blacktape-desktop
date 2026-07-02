import { useEffect, useMemo, useState } from "react";
import styles from "./search-bar.module.css";
import { invoke } from "@tauri-apps/api/core";
import { AiOutlineArrowRight, AiOutlineSearch } from "react-icons/ai";
import { BiDownload, BiFilterAlt } from "react-icons/bi";
import clsx from "clsx";
import { formatCompactNumber } from "@/shared/lib/number.ts";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";

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

type FilterCategory = "All" | ItemType;
type Platform = "All" | "Youtube" | "Bandcamp" | "Local";

const SearchBar = () => {
  const [query, setQuery] = useState<string>("");
  const [debouncedQuery, setDebouncedQuery] = useState<string>(query);

  const [isFilterMenuOpen, setIsFilterMenuOpen] = useState<boolean>(false);
  const [activeCategory, setActiveCategory] = useState<FilterCategory>("All");
  const [activePlatform, setActivePlatform] = useState<Platform>("All");

  const { searchResults, setSearchQuery, executeSearch, setActiveView } =
    useLibraryStore();

  const handleBrowseAll = (e: React.MouseEvent) => {
    e.preventDefault();
    setActiveView("SEARCH_RESULTS");
  };

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedQuery(query);
    }, 200);

    return () => clearTimeout(handler);
  }, [query]);

  useEffect(() => {
    setSearchQuery(debouncedQuery);
    executeSearch(debouncedQuery);
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
    <div className={styles.container}>
      <div className={styles.searchFieldWrapper}>
        <AiOutlineSearch size={18} className={styles.searchIcon} />

        <input
          className={styles.input}
          type="text"
          placeholder="Search music..."
          value={query}
          onInput={(e) => setQuery(e.currentTarget.value)}
        />

        <button
          type="button"
          className={clsx(
            styles.filterToggleBtn,
            (activeCategory !== "All" || activePlatform !== "All") &&
              styles.filterActive,
          )}
          onClick={() => setIsFilterMenuOpen(!isFilterMenuOpen)}
          onBlur={() => setTimeout(() => setIsFilterMenuOpen(false), 200)}
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

      {/* Standard Overlay Panel for Search Suggestion Items */}
      {filteredResults.length > 0 && (
        <div className={styles.suggestions}>
          <div className={styles.suggestionsList}>
            {filteredResults.slice(0, 20).map((suggestion, index) => (
              <div key={index} className={styles.suggestion}>
                <img
                  className={clsx(styles.cover, {
                    [styles.artist]: suggestion.item_type === ItemType.Artist,
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
                            {formatCompactNumber(suggestion.subscriber_count)}{" "}
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
                  <button
                    className={styles.downloadButton}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => {
                      invoke("download", {
                        platform: suggestion.platform,
                        downloadType: suggestion.item_type,
                        url: suggestion.item_url_path,
                      });
                    }}
                  >
                    {suggestion.item_type !== ItemType.Artist && (
                      <BiDownload size={20} />
                    )}
                  </button>
                </div>
              </div>
            ))}
          </div>
          <a
            className={styles.browseButton}
            href="#"
            onClick={handleBrowseAll}
            onMouseDown={(e) => e.preventDefault()}
          >
            Browse all results <AiOutlineArrowRight size={20} />
          </a>
        </div>
      )}
    </div>
  );
};

export default SearchBar;
