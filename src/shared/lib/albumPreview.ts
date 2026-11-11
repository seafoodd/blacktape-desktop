/**
 * Album preview: the track list of an album/set before downloading. Types mirror `AlbumPreview` in
 * `src-tauri/src/download/album_preview.rs` (serde, snake_case); the helpers are pure.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  albumStateFromTracks,
  type DownloadState,
  type RemoteRef,
} from "@/shared/lib/libraryIndex.ts";
import { formatTotalDuration } from "@/shared/lib/trackDuration.ts";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { Platform } from "@/shared/types.ts";

export interface AlbumTrackInfo {
  /** Position in the album (0-based, also for unavailable rows). */
  index: number;
  title: string;
  artists: string[];
  track_number: number | null;
  /** Page url of the track (the download selects tracks by it); empty when unavailable. */
  url: string;
  available: boolean;
  /** Why the track is unavailable. */
  note: string | null;
  /** Length in ms when the platform provides it. */
  duration_ms?: number | null;
}

export interface AlbumPreview {
  platform: Platform;
  url: string;
  title: string;
  artist: string;
  cover_url: string | null;
  year: number | null;
  genres: string[];
  tracks: AlbumTrackInfo[];
  /** Sum of the known track durations in ms; null when no track has one. */
  total_duration_ms?: number | null;
}

/** Fetches the track list of the album at `url`. Rejects with a message string. */
export const getAlbumPreview = (platform: Platform, url: string): Promise<AlbumPreview> =>
  invoke<AlbumPreview>("get_album_preview", { platform, url });

/** The tracks that can be selected and downloaded, in album order. */
export function selectableTracks(preview: Pick<AlbumPreview, "tracks">): AlbumTrackInfo[] {
  return preview.tracks.filter((t) => t.available && t.url !== "");
}

/** Indexes of the tracks that can be selected and downloaded, in album order. */
export function selectableIndexes(preview: Pick<AlbumPreview, "tracks">): number[] {
  return selectableTracks(preview).map((t) => t.index);
}

/** Page urls of the selected tracks, in album order (unavailable or unknown indexes are dropped). */
export function selectedUrls(
  preview: Pick<AlbumPreview, "tracks">,
  selected: ReadonlySet<number>,
): string[] {
  return preview.tracks
    .filter((t) => selected.has(t.index) && t.available && t.url !== "")
    .map((t) => t.url);
}

/** "Download album", "Download track" or "Download 3 of 10 tracks". */
export function downloadLabel(selectedCount: number, availableCount: number): string {
  if (selectedCount === 0 || selectedCount >= availableCount) {
    return availableCount === 1 ? "Download track" : "Download album";
  }
  return selectedCount === 1
    ? "Download 1 track"
    : `Download ${selectedCount} of ${availableCount} tracks`;
}

/** What the main button of the album preview does. */
export type AlbumAction =
  | { kind: "download"; label: string; urls?: undefined }
  | { kind: "download-missing"; label: string; urls: string[] }
  | { kind: "go-to-album"; label: "Go to album"; urls?: undefined };

/**
 * The main button: with a selection it downloads the selection (`downloadLabel`); without one it depends on what
 * the library already has of the album: everything -> "Go to album", some tracks -> download only the missing
 * ones, nothing -> the whole album. `states` is the library state of every track url. Pure.
 */
export function albumAction(
  preview: Pick<AlbumPreview, "tracks">,
  states: Readonly<Record<string, DownloadState>>,
  selectedCount: number,
): AlbumAction {
  const tracks = selectableTracks(preview);
  if (selectedCount > 0 || tracks.length === 0) {
    return { kind: "download", label: downloadLabel(selectedCount, tracks.length) };
  }
  const missing = tracks.filter((t) => (states[t.url] ?? "none") !== "downloaded").map((t) => t.url);
  if (missing.length === 0) return { kind: "go-to-album", label: "Go to album" };
  if (missing.length === tracks.length) return { kind: "download", label: downloadLabel(0, tracks.length) };
  return {
    kind: "download-missing",
    label: missing.length === 1 ? "Download missing track" : "Download missing tracks",
    urls: missing,
  };
}

/** "2020 · Ambient · 1 hr 5 min · 10 tracks": the album info line followed by the track count. */
export function albumMetaLine(
  preview: Pick<AlbumPreview, "year" | "genres" | "tracks"> &
    Partial<Pick<AlbumPreview, "total_duration_ms">>,
): string {
  return [albumInfoLine(preview), trackCountLabel(preview)].filter((p) => p !== "").join(" · ");
}

/** "10 tracks", "1 track", "10 tracks · 2 unavailable". */
export function trackCountLabel(preview: Pick<AlbumPreview, "tracks">): string {
  const total = preview.tracks.length;
  const unavailable = preview.tracks.filter((t) => !t.available).length;
  const base = `${total} ${total === 1 ? "track" : "tracks"}`;
  return unavailable > 0 ? `${base} · ${unavailable} unavailable` : base;
}

/** "2020 · Ambient, Drone · 1 hr 5 min" (parts that are missing are skipped). */
export function albumInfoLine(
  preview: Pick<AlbumPreview, "year" | "genres"> & Partial<Pick<AlbumPreview, "total_duration_ms">>,
): string {
  return [
    preview.year ? String(preview.year) : "",
    preview.genres.join(", "),
    formatTotalDuration(preview.total_duration_ms),
  ]
    .filter((p) => p !== "")
    .join(" · ");
}

/** The track as a search item, so the stream button can play it (and queue the tracks after it). */
export function trackAsStreamItem(
  preview: Pick<AlbumPreview, "platform" | "title" | "artist" | "cover_url"> & Partial<Pick<AlbumPreview, "url">>,
  track: AlbumTrackInfo,
): SearchSuggestion {
  return {
    item_type: ItemType.Track,
    name: track.title,
    band_name: track.artists[0] || preview.artist,
    album_name: preview.title,
    album_url: preview.url,
    item_url_path: track.url,
    img: preview.cover_url ?? "",
    duration: typeof track.duration_ms === "number" ? Math.round(track.duration_ms / 1000) : undefined,
    platform: preview.platform,
  };
}

/** Library-index refs of the selectable tracks (what the status hooks are asked about). */
export function trackRefs(preview: Pick<AlbumPreview, "platform" | "artist" | "title" | "tracks">): RemoteRef[] {
  return selectableTracks(preview).map((t) => ({
    platform: preview.platform,
    url: t.url,
    artist: t.artists[0] || preview.artist,
    title: t.title,
    album: preview.title || undefined,
  }));
}

/**
 * Library state of the album, from the states of its selectable tracks (D7: the album preview derives it from
 * the tracks, which also covers albums whose tracks were downloaded one by one). Missing states count as "none".
 */
export function albumLibraryState(
  preview: Pick<AlbumPreview, "tracks">,
  states: Readonly<Record<string, DownloadState>>,
): DownloadState {
  return albumStateFromTracks(selectableTracks(preview).map((t) => states[t.url] ?? "none"));
}

/** Progress of a download task that serves some rows of this page. */
export type TrackTaskState = { phase: "running"; percent: number } | { phase: "done" };

/** Download task statuses that still do work (the library keeps other tasks only as history). */
const RUNNING_STATUSES: ReadonlySet<string> = new Set(["idle", "downloading", "processing"]);

const clampPercent = (value: number): number =>
  Number.isFinite(value) ? Math.min(100, Math.max(0, Math.round(value))) : 0;

/**
 * Rows served by the tasks of this page: track url -> state. `taskTracks` maps a task id to the track urls it
 * downloads; `active` is the library's `activeDownloads`. A failed or cancelled task serves no row (the row
 * shows its download button again); a completed one reports "done" until the library drops the task.
 */
export function trackTaskStates(
  taskTracks: Readonly<Record<string, readonly string[]>>,
  active: Readonly<Record<string, { status: string; overallPercent: number }>>,
): Record<string, TrackTaskState> {
  const out: Record<string, TrackTaskState> = {};
  for (const [taskId, urls] of Object.entries(taskTracks)) {
    const task = active[taskId];
    if (!task) continue;
    let state: TrackTaskState | undefined;
    if (RUNNING_STATUSES.has(task.status)) {
      state = { phase: "running", percent: clampPercent(task.overallPercent) };
    } else if (task.status === "completed") {
      state = { phase: "done" };
    }
    if (!state) continue;
    for (const url of urls) out[url] = state;
  }
  return out;
}

export type TrackRowStatus =
  | { kind: "unavailable" }
  | { kind: "downloaded" }
  | { kind: "running"; percent: number }
  | { kind: "download" };

/**
 * What the action slot of a track row shows. The library state wins over a running task: a track that is in the
 * library is downloaded even while another download of the album still runs.
 */
export function trackRowStatus(
  track: Pick<AlbumTrackInfo, "available" | "url">,
  libraryState: DownloadState | undefined,
  task: TrackTaskState | undefined,
): TrackRowStatus {
  if (!track.available || track.url === "") return { kind: "unavailable" };
  if (libraryState === "downloaded" || task?.phase === "done") return { kind: "downloaded" };
  if (task?.phase === "running") return { kind: "running", percent: task.percent };
  return { kind: "download" };
}
