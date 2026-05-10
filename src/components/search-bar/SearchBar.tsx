import { useEffect, useState } from "react";
import styles from "./search-bar.module.css";
import { invoke } from "@tauri-apps/api/core";
import { AiOutlineArrowRight } from "react-icons/ai";
import { BiDownload } from "react-icons/bi";

type SearchSuggestion = {
  item_type: string;
  name: string;
  band_name: string;
  album_name?: string;
  item_url_path: string;
  img: string;
};

const formatSuggestionType = (type: string): string => {
  if (type === "a") {
    return "ALBUM";
  } else if (type === "t") {
    return "TRACK";
  } else {
    return "";
  }
};

const SearchBar = () => {
  const [suggestions, setSuggestions] = useState<SearchSuggestion[] | null>(
    null,
  );
  const [query, setQuery] = useState<string>("");
  const [debouncedQuery, setDebouncedQuery] = useState<string>(query);

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedQuery(query);
    }, 200);

    return () => {
      clearTimeout(handler);
    };
  }, [query]);

  useEffect(() => {
    if (debouncedQuery.length > 2) {
      console.log("Searching for:", debouncedQuery);
      invoke<SearchSuggestion[]>("get_search_suggestions", { query }).then(
        (res) => setSuggestions(res),
      );
    } else {
      setSuggestions(null);
    }
  }, [debouncedQuery]);

  return (
    <div className={styles.container}>
      <input
        className={styles.input}
        type="text"
        value={query}
        onInput={(e) => setQuery(e.currentTarget.value)}
      />

      {suggestions && (
        <div className={styles.suggestions}>
          {suggestions.slice(0, 7).map((suggestion, index) => (
            <div key={index} className={styles.suggestion}>
              <img className={styles.cover} src={suggestion.img} alt="cover" />
              <div className={styles.rightBlock}>
                <p className={styles.title}>{suggestion.name}</p>
                <p className={styles.artist}>by {suggestion.band_name}</p>
                <p className={styles.type}>
                  {formatSuggestionType(suggestion.item_type)}
                </p>
              </div>
              <div className={styles.tools}>
                <button
                  className={styles.downloadButton}
                  onClick={() => console.log("download")}
                >
                  <BiDownload size={20} />
                </button>
              </div>
            </div>
          ))}
          <a className={styles.browseButton} href="">
            Browse all results <AiOutlineArrowRight size={20} />
          </a>
        </div>
      )}
    </div>
  );
};

export default SearchBar;
