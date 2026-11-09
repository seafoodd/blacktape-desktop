/**
 * Album preview: the track list of an album/set before downloading. Types mirror `AlbumPreview` in
 * `src-tauri/src/download/album_preview.rs` (serde, snake_case); the helpers are pure.
 */
import { invoke } from "@tauri-apps/api/core";
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
}

/** Fetches the track list of the album at `url`. Rejects with a message string. */
export const getAlbumPreview = (platform: Platform, url: string): Promise<AlbumPreview> =>
  invoke<AlbumPreview>("get_album_preview", { platform, url });

/** Indexes of the tracks that can be selected and downloaded, in album order. */
export function selectableIndexes(preview: Pick<AlbumPreview, "tracks">): number[] {
  return preview.tracks.filter((t) => t.available && t.url !== "").map((t) => t.index);
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

/** "10 tracks", "1 track", "10 tracks · 2 unavailable". */
export function trackCountLabel(preview: Pick<AlbumPreview, "tracks">): string {
  const total = preview.tracks.length;
  const unavailable = preview.tracks.filter((t) => !t.available).length;
  const base = `${total} ${total === 1 ? "track" : "tracks"}`;
  return unavailable > 0 ? `${base} · ${unavailable} unavailable` : base;
}

/** "2020 · Ambient, Drone" (parts that are missing are skipped). */
export function albumInfoLine(preview: Pick<AlbumPreview, "year" | "genres">): string {
  return [preview.year ? String(preview.year) : "", preview.genres.join(", ")]
    .filter((p) => p !== "")
    .join(" · ");
}

/** The track as a search item, so the stream button can play it (and queue the tracks after it). */
export function trackAsStreamItem(
  preview: Pick<AlbumPreview, "platform" | "title" | "artist" | "cover_url">,
  track: AlbumTrackInfo,
): SearchSuggestion {
  return {
    item_type: ItemType.Track,
    name: track.title,
    band_name: track.artists[0] || preview.artist,
    album_name: preview.title,
    item_url_path: track.url,
    img: preview.cover_url ?? "",
    platform: preview.platform,
  };
}
