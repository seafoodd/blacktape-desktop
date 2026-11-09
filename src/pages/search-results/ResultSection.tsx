import type { ReactNode } from "react";
import {
  ItemType,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type { SearchKind } from "@/shared/lib/searchFull.ts";
import type { SectionState } from "@/features/search/model/searchPageStore.ts";
import styles from "./search-results.module.css";
import {
  AlbumCard,
  ArtistCard,
  SectionSkeleton,
  TrackRow,
} from "./ResultItems.tsx";
import {
  formatCountLabel,
  type DownloadMap,
} from "./searchResultsFormat.ts";

interface ResultSectionProps {
  kind: SearchKind;
  title: string;
  emptyText: string;
  section: SectionState;
  /** Filtered and sorted items of this section (before the "All" tab preview limit). */
  visible: SearchSuggestion[];
  /** "All" tab: show at most this many and link to the tab. null = full list with "Show more". */
  previewLimit: number | null;
  downloads: DownloadMap;
  onSeeAll: () => void;
  onLoadMore: () => void;
  /** Re-runs the whole search (used when the section failed without any items). */
  onRetry: () => void;
  onResetFilters: () => void;
}

const StateBox = ({ message, children }: { message: string; children?: ReactNode }) => (
  <div className={styles.stateBox} role="status">
    <p>{message}</p>
    {children}
  </div>
);

export const ResultSection = ({
  kind,
  title,
  emptyText,
  section,
  visible,
  previewLimit,
  downloads,
  onSeeAll,
  onLoadMore,
  onRetry,
  onResetFilters,
}: ResultSectionProps) => {
  const { items, status, error, hasMore } = section;
  const loading = status === "idle" || status === "loading";
  // A first-page failure is status "error" with no items (full error box). A failed "Show more" keeps the items,
  // status "ready" and `error` set: that is shown inline next to Show more, which doubles as the retry.
  const failed = status === "error";
  const inlineError = items.length > 0 && error !== null;
  const filteredOut = !loading && items.length > 0 && visible.length === 0;
  const shown = previewLimit === null ? visible : visible.slice(0, previewLimit);
  const countText = loading ? null : formatCountLabel(visible.length, hasMore);
  const headingId = `search-section-${kind}`;

  let body: ReactNode;
  if (loading && items.length === 0) {
    body = <SectionSkeleton kind={kind} />;
  } else if (failed && items.length === 0) {
    body = (
      <StateBox message={error ?? "Could not load results."}>
        <button type="button" className={styles.actionBtn} onClick={onRetry}>
          Retry
        </button>
      </StateBox>
    );
  } else if (filteredOut) {
    body = (
      <StateBox message="No results match your filters.">
        <button type="button" className={styles.actionBtn} onClick={onResetFilters}>
          Reset filters
        </button>
      </StateBox>
    );
  } else if (items.length === 0) {
    body = <StateBox message={emptyText} />;
  } else {
    body = (
      <>
        {kind === ItemType.Track && (
          <div className={styles.trackList}>
            {shown.map((item, i) => (
              <TrackRow
                key={`${i}-${item.item_url_path}`}
                item={item}
                list={visible}
                progress={downloads.get(item.item_url_path)}
              />
            ))}
          </div>
        )}
        {kind === ItemType.Album && (
          <div className={styles.albumGrid}>
            {shown.map((item, i) => (
              <AlbumCard
                key={`${i}-${item.item_url_path}`}
                item={item}
                progress={downloads.get(item.item_url_path)}
              />
            ))}
          </div>
        )}
        {kind === ItemType.Artist && (
          <div className={styles.artistGrid}>
            {shown.map((item, i) => (
              <ArtistCard key={`${i}-${item.item_url_path}`} item={item} />
            ))}
          </div>
        )}

        {inlineError && (
          <p className={styles.inlineError} role="alert">
            {error}
          </p>
        )}

        {previewLimit !== null ? (
          (visible.length > previewLimit || hasMore) && (
            <div className={styles.moreRow}>
              <button type="button" className={styles.linkBtn} onClick={onSeeAll}>
                See all {countText}
              </button>
            </div>
          )
        ) : (
          hasMore && (
            <div className={styles.moreRow}>
              <button
                type="button"
                className={styles.actionBtn}
                disabled={status === "loadingMore"}
                onClick={onLoadMore}
              >
                {status === "loadingMore"
                  ? "Loading..."
                  : inlineError
                    ? "Retry Show more"
                    : "Show more"}
              </button>
            </div>
          )
        )}
      </>
    );
  }

  return (
    <section className={styles.section} aria-labelledby={headingId}>
      <div className={styles.sectionHeader}>
        <h3 id={headingId} className={styles.sectionTitle}>
          {title}
        </h3>
        {countText !== null && <span className={styles.sectionCount}>{countText}</span>}
      </div>
      {body}
    </section>
  );
};
