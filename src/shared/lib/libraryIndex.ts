/**
 * Library index: is a remote track/album already in the library, and which tracks of a library album are
 * missing. Types mirror `src-tauri/src/library_index/{status,commands}.rs` (serde, snake_case). FROZEN
 * contract of the library-sync wave 1 (see work/v3/PLAN.md): parts only add code, never change these shapes.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Platform } from "@/shared/types.ts";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";

export type DownloadState = "none" | "partial" | "downloaded";

/** A remote item the backend is asked about (page url of a track or an album). */
export interface RemoteRef {
  platform: Platform;
  url: string;
  /** Artist and title of a track: lets the backend recognise songs downloaded before the library kept page urls. */
  artist?: string;
  title?: string;
}

export interface AlbumStatus {
  state: DownloadState;
  /** Tracks of the album that are in the library. */
  owned: number;
  /** Tracks of the album (0 when the album has no known track list). */
  total: number;
}

/** One library album that has a known remote source (db only; albums without a source are absent). */
export interface LibraryAlbumSource {
  /** Title of the LIBRARY album (`Album.title` of the library page). */
  album: string;
  platform: Platform;
  /** Page url of the remote album. */
  source_url: string;
  cover_url: string | null;
  total: number;
  owned: number;
  missing: number;
}

/** The tracks of a library album that are not in the library (same row shape as the album preview). */
export interface MissingTracks {
  platform: Platform;
  source_url: string;
  album_title: string;
  album_artist: string;
  cover_url: string | null;
  tracks: AlbumTrackInfo[];
}

/** Library state of track urls. The answer is keyed by the url exactly as sent. */
export const getDownloadStatus = (items: RemoteRef[]): Promise<Record<string, DownloadState>> =>
  invoke<Record<string, DownloadState>>("get_download_status", { items });

/** Library state of album urls (from the albums the app downloaded before). Keyed by the url as sent. */
export const getAlbumStatus = (items: RemoteRef[]): Promise<Record<string, AlbumStatus>> =>
  invoke<Record<string, AlbumStatus>>("get_album_status", { items });

/** Where the library keeps an album downloaded from a platform page. */
export interface LibraryAlbumRef {
  album_artist: string;
  album: string;
}

/** The library album the remote album `url` was downloaded into; null when the app never downloaded it. */
export const getLibraryAlbum = (url: string): Promise<LibraryAlbumRef | null> =>
  invoke<LibraryAlbumRef | null>("get_library_album", { url });

/** Sources of the albums of an artist (one call per opened artist page). */
export const getArtistAlbumSources = (albumArtist: string): Promise<LibraryAlbumSource[]> =>
  invoke<LibraryAlbumSource[]>("get_artist_album_sources", { albumArtist });

/** Missing tracks of a library album; null when the album has no source. `refresh` re-reads the platform. */
export const getMissingTracks = (
  albumArtist: string,
  album: string,
  refresh = false,
): Promise<MissingTracks | null> =>
  invoke<MissingTracks | null>("get_missing_tracks", { albumArtist, album, refresh });

/** A platform album that may be the library album (best match first). Mirrors `library_index/candidates.rs`. */
export interface AlbumCandidate {
  platform: Platform;
  url: string;
  title: string;
  artist: string;
  cover_url: string | null;
  year: number | null;
  score: number;
}

/** Platform albums that may be the library album, for an album that has no stored source. */
export const findAlbumCandidates = (albumArtist: string, album: string): Promise<AlbumCandidate[]> =>
  invoke<AlbumCandidate[]>("find_album_candidates", { albumArtist, album });

/** Links the library album to the platform album at `url` (stores its track list as the album's source). */
export const linkAlbumSource = (
  albumArtist: string,
  album: string,
  platform: Platform,
  url: string,
): Promise<void> => invoke<void>("link_album_source", { albumArtist, album, platform, url });

/** Removes the link of the library album to its platform album (it can be linked again afterwards). */
export const unlinkAlbumSource = (albumArtist: string, album: string): Promise<void> =>
  invoke<void>("unlink_album_source", { albumArtist, album });

/** none / partial / downloaded from the states of an album's tracks. No tracks -> "none". Pure. */
export function albumStateFromTracks(states: readonly DownloadState[]): DownloadState {
  if (states.length === 0) return "none";
  const owned = states.filter((s) => s === "downloaded").length;
  if (owned === 0) return "none";
  return owned === states.length ? "downloaded" : "partial";
}

/** Cache key of a remote item in the index store (urls are compared as sent). */
export const refKey = (ref: RemoteRef): string => ref.url;
