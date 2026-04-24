import { useEffect, useState } from "react";
import styles from "./search-bar.module.css";
import { invoke } from "@tauri-apps/api/core";

const SearchBar = () => {
  const [suggestions, setSuggestions] = useState<string[] | null>(null);
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
      invoke<string[]>("get_search_suggestions", { query }).then((res) =>
        setSuggestions(res),
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
          {suggestions.map((suggestion, index) => (
            <div key={index} className={styles.suggestion}>{suggestion}</div>
          ))}
        </div>
      )}
    </div>
  );
};

export default SearchBar;
