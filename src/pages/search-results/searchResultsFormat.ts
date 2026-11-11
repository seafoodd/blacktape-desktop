/**
 * Pure presentational helpers of the search results page: durations, inputs, labels, download state.
 * No React and no runtime imports of the store (type-only imports are erased), so they are cheap to unit test.
 */
import {
  ItemType,
  type DownloadProgress,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type { Platform } from "@/shared/types.ts";
import type { SearchPageFilters } from "@/features/search/model/searchPageFilters.ts";
import { formatCompactNumber } from "@/shared/lib/search/number.ts";

/** Download progress keyed by the item url (`item_url_path`), first/active download wins. */
export type DownloadMap = ReadonlyMap<string, DownloadProgress>;

/** "m:ss" below an hour, "h:mm:ss" from an hour on. Invalid or negative input renders as "0:00". */
export function formatDurationSecs(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "0:00";
  const total = Math.floor(secs);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const ss = String(seconds).padStart(2, "0");
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${ss}`;
  }
  return `${minutes}:${ss}`;
}

/**
 * Duration column of a track row: "3:05", or "" when unknown. The column keeps its width either way, so rows
 * with and without a duration line up.
 */
export function formatDurationCell(secs: number | null | undefined): string {
  return typeof secs === "number" && Number.isFinite(secs) && secs >= 0
    ? formatDurationSecs(secs)
    : "";
}

/** Text shown in a duration input for a stored value (seconds). */
export function formatDurationField(secs: number | undefined): string {
  return secs === undefined ? "" : formatDurationSecs(secs);
}

/**
 * Parses a duration range input. Accepts "" (no bound, `value: undefined`), minutes ("3", "1.5"),
 * "m:ss" ("3:30", "90:00") and "h:mm:ss" ("1:02:03"). Returns null for anything else.
 */
export function parseDurationInput(
  text: string,
): { value: number | undefined } | null {
  const t = text.trim();
  if (t === "") return { value: undefined };

  if (/^\d+(\.\d+)?$/.test(t)) {
    return { value: Math.round(Number(t) * 60) };
  }

  const parts = t.split(":");
  if (parts.length < 2 || parts.length > 3) return null;
  if (!parts.every((p) => /^\d+$/.test(p))) return null;

  const nums = parts.map(Number);
  if (parts.length === 3) {
    const [h, m, s] = nums;
    if (m > 59 || s > 59) return null;
    return { value: h * 3600 + m * 60 + s };
  }
  const [m, s] = nums;
  if (s > 59) return null;
  return { value: m * 60 + s };
}

/** Year range input: a positive integer up to 9999, otherwise undefined (no bound). */
export function parseYearInput(text: string): number | undefined {
  const t = text.trim();
  if (t === "") return undefined;
  const n = Number(t);
  return Number.isInteger(n) && n > 0 && n <= 9999 ? n : undefined;
}

/** Human label of a lower-case platform id. */
export function platformLabel(platform: string | null | undefined): string {
  switch (platform) {
    case "youtube":
      return "YouTube";
    case "bandcamp":
      return "Bandcamp";
    case "soundcloud":
      return "SoundCloud";
    case "local":
      return "Local";
    default:
      return platform ?? "";
  }
}

/** Percent of a running download (0..100), 0 while unknown. */
export function downloadPercent(
  progress: Pick<DownloadProgress, "overallPercent" | "current" | "total">,
): number {
  const overall = progress.overallPercent;
  if (typeof overall === "number" && Number.isFinite(overall)) {
    return Math.min(100, Math.max(0, Math.round(overall)));
  }
  if (progress.total > 0 && progress.current > 0) {
    return Math.min(100, Math.round((progress.current / progress.total) * 100));
  }
  return 0;
}

/** True while a download occupies the item (failed and cancelled downloads free the button again). */
export function isDownloadBusy(
  progress: Pick<DownloadProgress, "status"> | undefined,
): boolean {
  return (
    progress !== undefined &&
    progress.status !== "failed" &&
    progress.status !== "cancelled"
  );
}

/**
 * Indexes downloads by item url. When several downloads share a url, a busy one beats an
 * old failed/cancelled/completed one; otherwise the first one wins.
 */
export function buildDownloadMap(
  list: readonly DownloadProgress[],
): Map<string, DownloadProgress> {
  const map = new Map<string, DownloadProgress>();
  for (const download of list) {
    const existing = map.get(download.url);
    if (!existing || (!isDownloadBusy(existing) && isDownloadBusy(download))) {
      map.set(download.url, download);
    }
  }
  return map;
}

const hasRange = (range: { min?: number; max?: number }): boolean =>
  range.min != null || range.max != null;

/** True when any filter (not the tab, not the sort) narrows the results. */
export function hasActiveFilters(
  filters: Pick<SearchPageFilters, "platform" | "year" | "durationSecs" | "text">,
): boolean {
  return (
    filters.platform !== "All" ||
    hasRange(filters.year) ||
    hasRange(filters.durationSecs) ||
    filters.text.trim() !== ""
  );
}

/** Filter patch that clears every filter but keeps the selected tab. */
export const CLEARED_FILTERS: Pick<
  SearchPageFilters,
  "platform" | "year" | "durationSecs" | "text"
> = {
  platform: "All",
  year: {},
  durationSecs: {},
  text: "",
};

/** Count shown next to a tab or section title: "12", or "12+" when more pages exist. */
export function formatCountLabel(count: number, hasMore: boolean): string {
  return `${count}${hasMore ? "+" : ""}`;
}

/** Drops empty and duplicate warning messages, keeps the first occurrence order. */
export function uniqueWarnings(list: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of list) {
    const message = raw.trim();
    if (message === "" || seen.has(message)) continue;
    seen.add(message);
    out.push(message);
  }
  return out;
}

/** Joins the present parts with a middle dot. */
export function joinMeta(
  parts: ReadonlyArray<string | null | undefined | false>,
): string {
  return parts.filter((p): p is string => typeof p === "string" && p !== "").join(" · ");
}

/** "1.2B plays", or null when the play count is unknown. */
export function formatPlays(count: number | undefined | null): string | null {
  return typeof count === "number" ? `${formatCompactNumber(count)} plays` : null;
}

/** "7.2M subscribers", or null when unknown or zero. */
export function formatSubscribers(count: number | undefined | null): string | null {
  return typeof count === "number" && count > 0
    ? `${formatCompactNumber(count)} subscribers`
    : null;
}

/** "7.2M monthly audience" / "1.7K subscribers" for a search item; null when the kind is unknown (never guess the label). */
export function formatAudience(
  count: number | undefined | null,
  kind: "subscribers" | "monthly_audience" | "followers" | null | undefined,
): string | null {
  if (typeof count !== "number" || count <= 0) return null;
  if (kind === "subscribers") return `${formatCompactNumber(count)} subscribers`;
  if (kind === "monthly_audience") return `${formatCompactNumber(count)} monthly audience`;
  if (kind === "followers") return `${formatCompactNumber(count)} followers`;
  return null;
}

/**
 * The album a track belongs to as an album search item (opens the album preview), or null when the track has no
 * album link. The artist is the first credited one: that is how albums are filed.
 */
export function trackAlbumItem(item: {
  name: string;
  band_name: string;
  album_name?: string | null;
  album_url?: string | null;
  img: string;
  platform: Platform;
}): SearchSuggestion | null {
  const url = item.album_url?.trim();
  const name = item.album_name?.trim();
  if (!url || !name) return null;
  return {
    item_type: ItemType.Album,
    name,
    band_name: item.band_name.split(",")[0].trim(),
    item_url_path: url,
    img: item.img,
    platform: item.platform,
  };
}

/** "Band - Album" line of a track row; empty parts are skipped. */
export function trackSubline(band: string, album?: string | null): string {
  return [band, album ?? ""].filter((p) => p.trim() !== "").join(" - ");
}

/**
 * Parameters of "Download" for a search item. A track that knows its album goes through the album pipeline (album
 * folder, cover, track number); everything else downloads the item itself.
 */
export function searchItemDownloadParams(item: SearchSuggestion): {
  platform: Platform;
  downloadType: ItemType;
  url: string;
  artistName: string;
  title: string;
  coverUrl: string;
  trackUrls?: string[];
} {
  const album = item.item_type === ItemType.Track ? trackAlbumItem(item) : null;
  if (album) {
    return {
      platform: item.platform,
      downloadType: ItemType.Album,
      url: album.item_url_path,
      artistName: album.band_name,
      title: album.name,
      coverUrl: item.img,
      trackUrls: [item.item_url_path],
    };
  }
  return {
    platform: item.platform,
    downloadType: item.item_type,
    url: item.item_url_path,
    artistName: item.band_name || (item.item_type === ItemType.Album ? item.name : "Unknown Artist"),
    title: item.name,
    coverUrl: item.img,
  };
}
