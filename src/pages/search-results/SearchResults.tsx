import { useMemo } from "react";
import styles from "./search-results.module.css";
import {
  ItemType,
  useLibraryStore,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type { SearchKind } from "@/shared/lib/searchFull.ts";
import { useSearchPageStore } from "@/features/search/model/searchPageStore.ts";
import { SearchFilterBar, type FilterTab } from "./SearchFilterBar.tsx";
import { ArtistProfileCard } from "./ArtistProfileCard.tsx";
import { ResultSection } from "./ResultSection.tsx";
import {
  ALL_TAB_PREVIEW_LIMIT,
  durationFilterEnabled,
  isKindShown,
  isProfileVisible,
  visibleSectionItems,
  yearFilterEnabled,
} from "./searchResultsView.ts";
import {
  buildDownloadMap,
  CLEARED_FILTERS,
  formatCountLabel,
  hasActiveFilters,
  uniqueWarnings,
} from "./searchResultsFormat.ts";

/**
 * Full search page (search v2). Reads everything from `useSearchPageStore`; downloads go through
 * `useLibraryStore`. Layout: header with platform warnings, sticky filter bar, artist profile card, sections.
 */
const SearchResults = () => {
  const query = useSearchPageStore((s) => s.query);
  const sections = useSearchPageStore((s) => s.sections);
  const profile = useSearchPageStore((s) => s.profile);
  const filters = useSearchPageStore((s) => s.filters);
  const sort = useSearchPageStore((s) => s.sort);
  const setFilters = useSearchPageStore((s) => s.setFilters);
  const setSort = useSearchPageStore((s) => s.setSort);
  const retry = useSearchPageStore((s) => s.retry);
  const loadMore = useSearchPageStore((s) => s.loadMore);

  const activeDownloads = useLibraryStore((s) => s.activeDownloads);
  const downloads = useMemo(
    () => buildDownloadMap(Object.values(activeDownloads)),
    [activeDownloads],
  );

  const visible: Record<SearchKind, SearchSuggestion[]> = useMemo(
    () => ({
      [ItemType.Track]: visibleSectionItems(sections[ItemType.Track].items, filters, sort, ItemType.Track, sections[ItemType.Track].copies),
      [ItemType.Album]: visibleSectionItems(sections[ItemType.Album].items, filters, sort, ItemType.Album, sections[ItemType.Album].copies),
      [ItemType.Artist]: visibleSectionItems(sections[ItemType.Artist].items, filters, sort, ItemType.Artist, sections[ItemType.Artist].copies),
    }),
    [sections, filters, sort],
  );

  const trimmed = query.trim();
  if (trimmed === "") {
    return (
      <div className={styles.container}>
        <div className={styles.hint}>
          Search for songs, albums or artists with the search bar above.
        </div>
      </div>
    );
  }

  const tab = filters.tab;
  const filtersActive = hasActiveFilters(filters);
  const resetFilters = () => setFilters(CLEARED_FILTERS);

  const countFor = (kind: SearchKind): string | null => {
    const s = sections[kind];
    if (s.status === "idle" || s.status === "loading") return null;
    return formatCountLabel(visible[kind].length, s.hasMore);
  };
  const tabs: FilterTab[] = [
    { value: ItemType.Track, label: "Tracks", count: countFor(ItemType.Track) },
    { value: ItemType.Album, label: "Albums", count: countFor(ItemType.Album) },
    { value: ItemType.Artist, label: "Artists", count: countFor(ItemType.Artist) },
    { value: "All", label: "All", count: null },
  ];

  const warnings = uniqueWarnings(
    Object.values(sections).flatMap((s) => s.warnings),
  );

  const profileArtist = profile.artist;
  const showProfile =
    profileArtist !== null &&
    isProfileVisible(tab, filters.platform, profile.status, profileArtist.platform);

  const showAll = tab === "All";

  const renderSection = (kind: SearchKind, title: string, emptyText: string) => (
    <ResultSection
      key={kind}
      kind={kind}
      title={title}
      emptyText={emptyText}
      section={sections[kind]}
      visible={visible[kind]}
      previewLimit={showAll ? ALL_TAB_PREVIEW_LIMIT[kind] : null}
      downloads={downloads}
      onSeeAll={() => setFilters({ tab: kind })}
      onLoadMore={() => void loadMore(kind)}
      onRetry={() => retry()}
      onResetFilters={resetFilters}
    />
  );

  return (
    <div className={styles.container}>
      <header className={styles.header}>
        <h2>
          Results for <span>"{trimmed}"</span>
        </h2>
        {warnings.length > 0 && (
          <ul className={styles.notices}>
            {warnings.map((message) => (
              <li key={message} className={styles.notice}>
                {message}
              </li>
            ))}
          </ul>
        )}
      </header>

      <SearchFilterBar
        tabs={tabs}
        filters={filters}
        sort={sort}
        yearEnabled={yearFilterEnabled(tab)}
        durationEnabled={durationFilterEnabled(tab)}
        hasActiveFilters={filtersActive}
        onFiltersChange={setFilters}
        onSortChange={setSort}
        onResetFilters={resetFilters}
      />

      <div className={styles.content}>
        {showProfile && profileArtist && (
          <ArtistProfileCard
            artist={profileArtist}
            profile={profile.status === "ready" ? profile.data : null}
            downloads={downloads}
          />
        )}

        {isKindShown(tab, ItemType.Track) &&
          renderSection(ItemType.Track, "Tracks", "No tracks found")}
        {isKindShown(tab, ItemType.Album) &&
          renderSection(ItemType.Album, "Albums", "No albums found")}
        {isKindShown(tab, ItemType.Artist) &&
          renderSection(ItemType.Artist, "Artists", "No artists found")}
      </div>
    </div>
  );
};

export default SearchResults;
