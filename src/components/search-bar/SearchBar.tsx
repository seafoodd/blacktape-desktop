import { useEffect, useState } from "react";
import styles from "./search-bar.module.css";
import { invoke } from "@tauri-apps/api/core";
import { AiOutlineArrowRight } from "react-icons/ai";
import { BiDownload } from "react-icons/bi";
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

const SearchBar = () => {
  const [query, setQuery] = useState<string>("");
  const [debouncedQuery, setDebouncedQuery] = useState<string>(query);

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

    return () => {
      clearTimeout(handler);
    };
  }, [query]);

  useEffect(() => {
    setSearchQuery(debouncedQuery);
    executeSearch(debouncedQuery);
  }, [debouncedQuery, setSearchQuery, executeSearch]);

  return (
    <div className={styles.container}>
      <input
        className={styles.input}
        type="text"
        value={query}
        onInput={(e) => setQuery(e.currentTarget.value)}
      />

      {searchResults.length > 0 && (
        <div className={styles.suggestions}>
          {searchResults.slice(0, 7).map((suggestion, index) => (
            <div key={index} className={styles.suggestion}>
              <img
                className={styles.cover}
                src={suggestion.img}
                referrerPolicy="no-referrer"
                crossOrigin=""
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
                          {formatCompactNumber(suggestion.subscriber_count)}
                        </span>
                      ) : (
                        0
                      )}{" "}
                      subscribers
                    </span>
                  ) : (
                    `by ${suggestion.band_name}`
                  )}
                </p>
                <p className={clsx(styles.type, "truncate")}>
                  {formatSuggestionType(suggestion.item_type)}
                </p>
              </div>
              <div className={styles.tools}>
                <button
                  className={styles.downloadButton}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => {
                    console.log("download", suggestion.item_url_path);
                    invoke("download", {
                      platform: "Youtube",
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
