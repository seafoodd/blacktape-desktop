/**
 * Tab-dependent view rules of the search results page (which section shows, which filter applies to which kind).
 * Pure: the only runtime import is the `ItemType` enum.
 */
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { SearchKind } from "@/shared/lib/search/searchFull.ts";
import {
  applyFilters,
  itemsForPlatform,
  normalizeText,
  type ItemCopy,
  sortItems,
  type PlatformChoice,
  type SearchPageFilters,
  type SearchPageSort,
  type TypeTab,
} from "@/features/search/model/searchPageFilters.ts";

/** Items shown per section on the "All" tab before the "See all" link. */
export const ALL_TAB_PREVIEW_LIMIT: Record<SearchKind, number> = {
  [ItemType.Track]: 8,
  [ItemType.Album]: 12,
  [ItemType.Artist]: 12,
};

/**
 * The year range only means something for albums (tracks and artists carry no year), the duration range only for
 * tracks. The filter state is shared by all tabs, so each section receives only the filters that apply to it;
 * otherwise an active year range would hide every track.
 */
export function filtersForKind(
  filters: SearchPageFilters,
  kind: SearchKind,
): SearchPageFilters {
  switch (kind) {
    case ItemType.Track:
      return { ...filters, year: {} };
    case ItemType.Album:
      return { ...filters, durationSecs: {} };
    default:
      return { ...filters, year: {}, durationSecs: {} };
  }
}

/** Filtered, then sorted items of one section (the tab is not applied here). */
export function visibleSectionItems(
  items: SearchSuggestion[],
  filters: SearchPageFilters,
  sort: SearchPageSort,
  kind: SearchKind,
  copies: ItemCopy[] = [],
): SearchSuggestion[] {
  const pool = itemsForPlatform(items, copies, filters.platform);
  return sortItems(applyFilters(pool, filtersForKind(filters, kind)), sort);
}

/** Whether the section of `kind` is part of the selected tab ("All" shows every section). */
export function isKindShown(tab: TypeTab, kind: SearchKind): boolean {
  return tab === "All" || tab === kind;
}

/** Year input is enabled on the Albums and All tabs. */
export function yearFilterEnabled(tab: TypeTab): boolean {
  return tab === "All" || tab === ItemType.Album;
}

/** Duration input is enabled on the Tracks and All tabs. */
export function durationFilterEnabled(tab: TypeTab): boolean {
  return tab === "All" || tab === ItemType.Track;
}

/**
 * The artist profile card is part of the Artists view (tab All or Artists) and is hidden when the platform filter
 * excludes the artist's platform. Only while loading or ready; a failed profile shows no card.
 */
export function isProfileVisible(
  tab: TypeTab,
  platform: PlatformChoice,
  status: "idle" | "loading" | "ready" | "error",
  artistPlatform: string | null | undefined,
): boolean {
  if (status !== "loading" && status !== "ready") return false;
  if (tab !== "All" && tab !== ItemType.Artist) return false;
  return platform === "All" || platform === artistPlatform;
}

const ALBUM_INTENT_RE = /\b(album|albums|ep|lp|discography|records?)\b/i;
const TRACK_INTENT_RE = /\b(song|songs|track|tracks|single|singles|lyrics?)\b/i;

/**
 * Decides whether the "All" tab should place the Albums section above Tracks.
 * True on explicit album-intent queries ("the doors albums") or when the top album covers every
 * query word (e.g. "death grips no love deep web") without an exact top-track title match. Pure.
 */
export function prefersAlbumsFirst(
  query: string,
  albums: readonly SearchSuggestion[],
  tracks: readonly SearchSuggestion[],
): boolean {
  if (albums.length === 0) return false;
  if (ALBUM_INTENT_RE.test(query)) return true;
  if (TRACK_INTENT_RE.test(query)) return false;

  const qNorm = normalizeText(query);
  const qTokens = qNorm.split(/\s+/).filter(Boolean);
  if (qTokens.length < 3) return false;

  const topAlbum = albums[0];
  const albumText = `${normalizeText(topAlbum.band_name)} ${normalizeText(topAlbum.name)}`;
  const albumTokens = new Set(albumText.split(/\s+/).filter(Boolean));
  const albumCoversQuery = qTokens.every((t) => albumTokens.has(t));
  if (!albumCoversQuery) return false;

  const topTrack = tracks[0];
  if (topTrack) {
    const trackText = `${normalizeText(topTrack.band_name)} ${normalizeText(topTrack.name)}`;
    const trackTitle = normalizeText(topTrack.name);
    if (trackTitle === qNorm || trackText === qNorm) return false;
  }
  return true;
}
