import { useEffect, useState } from "react";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { ItemType, useLibraryStore, type SearchSuggestion, type TrackPlacement } from "@/shared/store/libraryStore.ts";
import { searchFull } from "@/shared/lib/searchFull.ts";
import { platformLabel } from "@/shared/lib/platforms.ts";
import { formatTrackDuration } from "@/shared/lib/trackDuration.ts";
import styles from "./find-missing-modal.module.css";

interface FindAlternativeModalProps {
  artist: string;
  title: string;
  /** Platform that has no stream for the song; it is left out of the search. */
  excludePlatform?: string;
  /** The library album and song this download fills (it is filed there with these names and this number). */
  placement?: TrackPlacement;
}

/** Parameters of the single-track download of a search result (the library files it into the album by its title). */
export function alternativeDownloadParams(
  item: SearchSuggestion,
  artist: string,
  title: string,
  placement?: TrackPlacement,
) {
  return {
    platform: item.platform,
    downloadType: ItemType.Track,
    url: item.item_url_path,
    artistName: item.band_name || artist,
    title: item.name || title,
    coverUrl: item.img,
    placement,
  };
}

/** The search query of a song that is not available on its own platform. */
export const alternativeQuery = (artist: string, title: string): string => `${artist} ${title}`.trim();

/**
 * A song of an album has no audio stream on the album's platform: looks for it on YouTube and SoundCloud and
 * downloads the result the user picks (it lands in the album when its title matches the album's track).
 */
const FindAlternativeModal = ({ artist, title, excludePlatform, placement }: FindAlternativeModalProps) => {
  const closeModal = useModalStore((s) => s.closeModal);
  const startDownload = useLibraryStore((s) => s.startDownload);
  const [results, setResults] = useState<SearchSuggestion[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const platforms = (["youtube", "soundcloud"] as const).filter((p) => p !== excludePlatform);
    searchFull({ query: alternativeQuery(artist, title), kind: ItemType.Track, platforms: [...platforms] })
      .then((page) => {
        if (!cancelled) setResults(page.items.slice(0, 8));
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setResults([]);
      });
    return () => {
      cancelled = true;
    };
  }, [artist, title, excludePlatform]);

  const pick = (item: SearchSuggestion) => {
    void startDownload(alternativeDownloadParams(item, artist, title, placement));
    closeModal();
  };

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Find another source</h2>
      <span className={styles.text}>
        {results === null
          ? `Looking for "${title}" on YouTube and SoundCloud...`
          : results.length > 0
            ? `"${title}" has no stream on its platform. Download it from:`
            : "Nothing was found."}
      </span>
      {results && results.length > 0 && (
        <div className={styles.list}>
          {results.map((item) => (
            <button key={item.item_url_path} className={styles.candidate} onClick={() => pick(item)}>
              {item.img ? <img className={styles.cover} src={item.img} alt="" /> : <div className={styles.cover} />}
              <div className={styles.info}>
                <span className={styles.name}>{item.name}</span>
                <span className={styles.meta}>{item.band_name}</span>
                <span className={styles.meta}>
                  {platformLabel(item.platform)}
                  {item.duration ? ` · ${formatTrackDuration(item.duration * 1000)}` : ""}
                </span>
              </div>
            </button>
          ))}
        </div>
      )}
      {error && <span className={styles.error}>{error}</span>}
      <div className={styles.buttons}>
        <button className={styles.cancelButton} onClick={closeModal}>
          Cancel
        </button>
      </div>
    </div>
  );
};

export default FindAlternativeModal;
