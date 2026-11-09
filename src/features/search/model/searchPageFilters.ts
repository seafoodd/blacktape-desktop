/**
 * Pure helpers of the full search page: filter, sort, pick the profile artist, merge pages.
 * OWNER: part C (implement every function below; signatures are FROZEN, add helpers freely).
 * All functions are pure, never mutate their inputs and never throw on missing optional fields.
 * Tests: `searchPageFilters.test.ts` (part C writes them).
 *
 * Backend JSON uses `null` for absent optional values (Rust `Option`), so every optional number/string
 * is checked at runtime (`isNumber`, falsy string checks), not only by its TS type.
 */
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";

/** Tab order in the UI: Tracks, Albums, Artists, All. */
export type TypeTab = "All" | ItemType.Track | ItemType.Album | ItemType.Artist;
/** Lower-case platform ids as used by `SearchSuggestion.platform`. */
export type PlatformChoice = "All" | "youtube" | "bandcamp" | "soundcloud";

export interface NumberRange {
  min?: number;
  max?: number;
}

export interface SearchPageFilters {
  tab: TypeTab;
  platform: PlatformChoice;
  /** Release year, inclusive. Items WITHOUT a year are kept only when the range is empty (no min and no max). */
  year: NumberRange;
  /** Track duration in seconds, inclusive. Items without a duration are kept only when the range is empty. */
  durationSecs: NumberRange;
  /** Case/diacritics-insensitive substring over name + band_name + album_name; empty = no filter. */
  text: string;
}

export type SortKey = "relevance" | "name" | "year" | "popularity" | "duration";
export type SortDir = "asc" | "desc";
export interface SearchPageSort {
  key: SortKey;
  dir: SortDir;
}

export const DEFAULT_FILTERS: SearchPageFilters = {
  tab: "All",
  platform: "All",
  year: {},
  durationSecs: {},
  text: "",
};

/** "relevance" asc = backend order (best first); "relevance" desc = reversed. */
export const DEFAULT_SORT: SearchPageSort = { key: "relevance", dir: "asc" };

const isNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);

const isString = (value: unknown): value is string => typeof value === "string";

/**
 * Comparable form of a text: lower-case, diacritics removed, apostrophes dropped, every other run of
 * non-letter/non-digit characters turned into one space, trimmed. "Beyoncé" -> "beyonce",
 * "AC/DC" -> "ac dc", "Don't" -> "dont". Returns "" for null/undefined or punctuation-only input.
 */
export function normalizeText(value: string | null | undefined): string {
  return (value ?? "")
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase()
    .replace(/['’`]/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

/** A range bound counts only when it is a finite number (undefined, null and NaN mean "no bound"). */
const hasBound = (range: NumberRange): boolean =>
  isNumber(range.min) || isNumber(range.max);

function inRange(value: unknown, range: NumberRange): boolean {
  if (!hasBound(range)) return true;
  if (!isNumber(value)) return false;
  if (isNumber(range.min) && value < range.min) return false;
  if (isNumber(range.max) && value > range.max) return false;
  return true;
}

function matchesText(item: SearchSuggestion, text: string): boolean {
  const trimmed = text.trim();
  if (trimmed === "") return true;
  const fields = [item.name, item.band_name, item.album_name]
    .filter(isString)
    .join(" ");
  const needle = normalizeText(trimmed);
  // Punctuation-only queries (e.g. the band "!!!") have no normalized form: match the raw text instead.
  if (needle === "") return fields.toLowerCase().includes(trimmed.toLowerCase());
  return normalizeText(fields).includes(needle);
}

/** A copy of a listed item on another platform (mirrors `PlatformCopy` of `searchFull.ts`). */
export interface ItemCopy {
  of: string;
  item: SearchSuggestion;
}

/**
 * The list as it looks for one platform: an item of another platform is replaced, in place, by its copy on
 * `platform` when there is one (the search merges the same album from several platforms into one item, so without
 * this the YouTube filter would hide an album that is also on Bandcamp). "All" returns `items` unchanged. Pure.
 */
export function itemsForPlatform(
  items: SearchSuggestion[],
  copies: ItemCopy[],
  platform: PlatformChoice,
): SearchSuggestion[] {
  if (platform === "All" || copies.length === 0) return items;
  const byOrigin = new Map<string, SearchSuggestion>();
  for (const copy of copies) {
    if (copy.item.platform === platform && !byOrigin.has(copy.of)) byOrigin.set(copy.of, copy.item);
  }
  return items.map((item) =>
    item.platform === platform ? item : (byOrigin.get(item.item_url_path) ?? item),
  );
}

/** Applies platform / year / duration / text filters (NOT the tab; the tab selects which section list is shown). */
export function applyFilters(
  items: SearchSuggestion[],
  filters: SearchPageFilters,
): SearchSuggestion[] {
  return items.filter(
    (item) =>
      (filters.platform === "All" || item.platform === filters.platform) &&
      inRange(item.year, filters.year) &&
      inRange(item.duration, filters.durationSecs) &&
      matchesText(item, filters.text),
  );
}

type SortValue = string | number | undefined;

/** Value used by the sort key; undefined when the item does not have it (sorted last). */
function sortValue(item: SearchSuggestion, key: SortKey): SortValue {
  switch (key) {
    case "name": {
      const name = normalizeText(item.name);
      return name === "" ? undefined : name;
    }
    case "year":
      return isNumber(item.year) ? item.year : undefined;
    case "popularity":
      return popularityOf(item);
    case "duration":
      return isNumber(item.duration) ? item.duration : undefined;
    default:
      return undefined;
  }
}

function compareDefined(a: string | number, b: string | number): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  const left = String(a);
  const right = String(b);
  return left < right ? -1 : left > right ? 1 : 0;
}

/** Missing values always go last, whatever the direction. */
function compareValues(a: SortValue, b: SortValue, sign: number): number {
  if (a === undefined && b === undefined) return 0;
  if (a === undefined) return 1;
  if (b === undefined) return -1;
  return compareDefined(a, b) * sign;
}

/**
 * Stable sort. popularity = view_count for tracks, subscriber_count for artists, undefined for albums.
 * Items missing the sort value (no year / duration / popularity) go LAST in both directions.
 * name: case/diacritics-insensitive compare of `name`.
 */
export function sortItems(
  items: SearchSuggestion[],
  sort: SearchPageSort,
): SearchSuggestion[] {
  if (sort.key === "relevance") {
    const ordered = [...items];
    return sort.dir === "desc" ? ordered.reverse() : ordered;
  }
  const sign = sort.dir === "desc" ? -1 : 1;
  return items
    .map((item) => ({ item, value: sortValue(item, sort.key) }))
    .sort((a, b) => compareValues(a.value, b.value, sign))
    .map(({ item }) => item);
}

/** Value used by popularity sort (see above); undefined when unknown. */
export function popularityOf(item: SearchSuggestion): number | undefined {
  switch (item.item_type) {
    case ItemType.Track:
      return isNumber(item.view_count) ? item.view_count : undefined;
    case ItemType.Artist:
      return isNumber(item.subscriber_count) ? item.subscriber_count : undefined;
    default:
      return undefined;
  }
}

/**
 * Chooses the artist for the profile card from the ranked artists list (backend order), or null.
 * Rule (D17): normalize both (lower-case, strip diacritics/punctuation, collapse spaces). Candidate when
 * name === query, OR query starts with `name + " "` (e.g. "daft punk one more time"), OR (query.length >= 3 and
 * name starts with query). Among candidates prefer exact name match, then the earliest in the list. null if none.
 * Non-artist items and items without a name are ignored.
 */
export function pickProfileArtist(
  query: string,
  artists: SearchSuggestion[],
): SearchSuggestion | null {
  const q = normalizeText(query);
  if (q === "") return null;

  let best: SearchSuggestion | null = null;
  let bestIsExact = false;
  for (const artist of artists) {
    if (artist.item_type !== ItemType.Artist) continue;
    const name = normalizeText(artist.name);
    if (name === "") continue;
    const exact = name === q;
    const candidate =
      exact || q.startsWith(`${name} `) || (q.length >= 3 && name.startsWith(q));
    if (!candidate) continue;
    // Earlier candidates win ties; an exact match beats any earlier prefix candidate.
    if (best === null || (exact && !bestIsExact)) {
      best = artist;
      bestIsExact = exact;
    }
  }
  return best;
}

const nameKey = (item: SearchSuggestion): string =>
  `${item.item_type}|${normalizeText(item.name)}|${normalizeText(item.band_name)}`;

/**
 * Appends `incoming` to `existing`, skipping items whose `item_url_path` (or name+band when empty) already exists.
 * Duplicates inside `incoming` are skipped too. Never mutates; `existing` is kept as it is.
 */
export function mergePages(
  existing: SearchSuggestion[],
  incoming: SearchSuggestion[],
): SearchSuggestion[] {
  const seenUrls = new Set<string>();
  const seenNames = new Set<string>();
  const remember = (item: SearchSuggestion) => {
    if (item.item_url_path) seenUrls.add(item.item_url_path);
    seenNames.add(nameKey(item));
  };
  existing.forEach(remember);

  const merged = [...existing];
  for (const item of incoming) {
    if (item.item_url_path) {
      if (seenUrls.has(item.item_url_path)) continue;
    } else if (seenNames.has(nameKey(item))) {
      continue;
    }
    remember(item);
    merged.push(item);
  }
  return merged;
}
