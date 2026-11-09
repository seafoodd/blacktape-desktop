import { create } from "zustand";
import { isNoStreamFailure } from "./downloadFailure.ts";
import {
  getArtistAlbumSources,
  getMissingTracks,
  unlinkAlbumSource,
  type LibraryAlbumSource,
  type MissingTracks,
} from "@/shared/lib/libraryIndex.ts";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import { createLatestGuard, type LatestGuard } from "@/shared/lib/searchGuard.ts";
import {
  ItemType,
  useLibraryStore,
  type DownloadProgress,
  type StartDownloadParams,
} from "@/shared/store/libraryStore.ts";

/**
 * Missing songs of the library albums of the selected artist: the tracks of a remote album that the library does
 * not have yet. Albums are keyed by their library title (`Album.title`), as the backend answers them.
 */

function hasOwn(record: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

/** Sources by library album title. */
export function sourcesByAlbum(
  list: readonly LibraryAlbumSource[],
): Record<string, LibraryAlbumSource> {
  const sources: Record<string, LibraryAlbumSource> = {};
  for (const source of list) sources[source.album] = source;
  return sources;
}

/** The source of a library album, or undefined. Own keys only: a title such as "constructor" finds nothing. */
export function sourceOfAlbum(
  sources: Record<string, LibraryAlbumSource>,
  album: string,
): LibraryAlbumSource | undefined {
  return hasOwn(sources, album) ? sources[album] : undefined;
}

/** `shown` with `album` toggled. A new Set: store state is never mutated. */
export function toggleShown(shown: ReadonlySet<string>, album: string): Set<string> {
  const next = new Set(shown);
  if (next.has(album)) next.delete(album);
  else next.add(album);
  return next;
}

/** The shown albums that still have a source. An album that lost its source has nothing to list. */
export function pruneShown(
  shown: ReadonlySet<string>,
  sources: Record<string, LibraryAlbumSource>,
): Set<string> {
  return new Set([...shown].filter((album) => hasOwn(sources, album)));
}

/** The missing tracks that can be downloaded (the backend already filters, this keeps the rule in one place). */
export function playableMissingTracks(missing: Pick<MissingTracks, "tracks">): AlbumTrackInfo[] {
  return missing.tracks.filter((t) => t.available && t.url !== "");
}

export function missingTrackUrls(missing: Pick<MissingTracks, "tracks">): string[] {
  return playableMissingTracks(missing).map((t) => t.url);
}

/** Album download (D13) of some tracks of a remote album, titled and filed like the library album. */
export function missingDownloadParams(
  missing: MissingTracks,
  trackUrls: string[],
): StartDownloadParams {
  return {
    platform: missing.platform,
    downloadType: ItemType.Album,
    url: missing.source_url,
    artistName: missing.album_artist,
    title: missing.album_title,
    coverUrl: missing.cover_url ?? undefined,
    trackUrls,
  };
}

/**
 * The songs whose album download failed, for the failed downloads that were not handled yet. A failed download
 * means every song of it failed (the backend only fails when nothing could be downloaded), so all its pending
 * songs fail with its message. `noStream` is true when that is the "no audio stream" error of a single song (with
 * several songs the failing one is not known, so nothing is marked as having no stream).
 */
export function failedSongs(
  pending: ReadonlyMap<string, string>,
  downloads: Record<string, Pick<DownloadProgress, "id" | "url" | "status" | "message">>,
  handled: ReadonlySet<string>,
): { songs: { url: string; album: string; message: string; noStream: boolean }[]; handledIds: string[] } {
  const songs: { url: string; album: string; message: string; noStream: boolean }[] = [];
  const handledIds: string[] = [];
  for (const [key, d] of Object.entries(downloads)) {
    const id = d.id ?? key;
    if (d.status !== "failed" || handled.has(id)) continue;
    handledIds.push(id);
    const urls = [...pending].filter(([, album]) => album === d.url).map(([song]) => song);
    const message = d.message ?? "";
    for (const url of urls) {
      songs.push({ url, album: d.url, message, noStream: urls.length === 1 && isNoStreamFailure(message) });
    }
  }
  return { songs, handledIds };
}

/**
 * The songs whose failure can be forgotten: they failed in the album `albumUrl`, and the album's missing songs no
 * longer list them (they were downloaded another way). `failureAlbum` maps a failed song to its album's url.
 */
export function recoveredSongs(
  failureAlbum: Readonly<Record<string, string>>,
  albumUrl: string,
  missingUrls: ReadonlySet<string>,
): string[] {
  return Object.entries(failureAlbum)
    .filter(([song, album]) => album === albumUrl && !missingUrls.has(song))
    .map(([song]) => song);
}

/**
 * `source` with `missing` lowered by the missing songs that cannot be downloaded (no stream). Uses the loaded
 * missing songs when there are any (a song that was downloaded another way no longer counts as unavailable).
 */
export function withoutUnavailable(
  source: LibraryAlbumSource,
  loaded: Pick<MissingTracks, "tracks"> | null | undefined,
  unavailable: ReadonlySet<string>,
): LibraryAlbumSource {
  if (!loaded) return source;
  const dead = loaded.tracks.filter((t) => unavailable.has(t.url)).length;
  return dead === 0 ? source : { ...source, missing: Math.max(0, source.missing - dead) };
}

const UNAVAILABLE_KEY = "blacktape.noStreamTracks";

function loadUnavailable(): Set<string> {
  try {
    const raw = localStorage.getItem(UNAVAILABLE_KEY);
    const list: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(list) ? list.filter((u): u is string => typeof u === "string") : []);
  } catch {
    return new Set();
  }
}

function saveUnavailable(urls: ReadonlySet<string>) {
  try {
    localStorage.setItem(UNAVAILABLE_KEY, JSON.stringify([...urls]));
  } catch {
    /* storage unavailable: the marks last until the app closes */
  }
}

/** Album urls of the downloads that are running or queued. */
export function busyAlbumUrls(
  activeDownloads: Record<string, Pick<DownloadProgress, "url" | "status">>,
): Set<string> {
  const busy = new Set<string>();
  for (const d of Object.values(activeDownloads)) {
    if (d.status === "downloading" || d.status === "idle" || d.status === "processing") {
      busy.add(d.url);
    }
  }
  return busy;
}

/**
 * The pending songs (song url -> album url) whose album still has a busy download. A song leaves the map when its
 * album download completes, fails or is cancelled.
 */
export function prunePending(
  pending: ReadonlyMap<string, string>,
  busyAlbums: ReadonlySet<string>,
): Map<string, string> {
  const next = new Map<string, string>();
  for (const [track, album] of pending) {
    if (busyAlbums.has(album)) next.set(track, album);
  }
  return next;
}

export interface MissingTracksState {
  /** The artist the state describes (the selected tab). Everything else belongs to this artist. */
  artist: string | null;
  /** Library albums of the artist that have a known remote source, by album title. */
  sources: Record<string, LibraryAlbumSource>;
  /** Albums whose missing songs are listed under them. */
  shown: Set<string>;
  /** Missing tracks by album title. `undefined`: not loaded; `null`: the album has no source. */
  tracks: Record<string, MissingTracks | null | undefined>;
  /**
   * Songs whose album download was started and has not ended (song url -> album url). Blocks a second download of
   * the same song (double click, "Download missing" right after a song download).
   */
  pendingUrls: Map<string, string>;
  /** Songs the platform lists but has no stream for (their download failed that way): cannot be downloaded from it. */
  unavailable: Set<string>;
  /** Songs whose last download failed (song url -> error message); cleared when the download is retried. */
  failures: Record<string, string>;
  /** Album url of each failed song (song url -> album url), to forget the failure once the song is downloaded. */
  failureAlbum: Record<string, string>;
  /** Starts the download of one missing song again after it failed. */
  retry: (album: string, track: AlbumTrackInfo) => Promise<void>;
  /** Loads the sources of `artist`; reloads the missing songs of the albums that are shown. */
  loadSources: (artist: string) => Promise<void>;
  /** Shows or hides the missing songs of `album` (loads them when shown). */
  toggle: (album: string) => void;
  /** Shows the missing songs of `album` (loads them). */
  show: (album: string) => void;
  /** Removes the link of `album` to its remote album and forgets its missing songs. */
  unlink: (artist: string, album: string) => Promise<void>;
  /** Starts one album download of the missing songs of `album` that are not already downloading. */
  downloadMissing: (album: string) => Promise<void>;
  /** Starts the album download of one missing song of `album`. Ignored when it is no longer missing or is downloading. */
  downloadOne: (album: string, track: AlbumTrackInfo) => Promise<void>;
  /** Starts one album download of the given missing songs of `album` (those no longer missing or pending are skipped). */
  downloadMany: (album: string, tracks: readonly AlbumTrackInfo[]) => Promise<void>;
}

// Only the latest answer may update the sources of the artist. The missing songs are guarded per album, so
// two albums loading at once do not discard each other.
const sourcesGuard = createLatestGuard();
const missingGuards = new Map<string, LatestGuard>();

function missingGuard(album: string): LatestGuard {
  let guard = missingGuards.get(album);
  if (!guard) {
    guard = createLatestGuard();
    missingGuards.set(album, guard);
  }
  return guard;
}

export const useMissingTracksStore = create<MissingTracksState>((set, get) => {
  /**
   * Reads the missing songs of `album` from the backend (always fresh: a download may have just added some).
   * Stores them unless a newer read of the album or another artist started meanwhile. Undefined on error.
   */
  const reloadMissing = async (album: string): Promise<MissingTracks | null | undefined> => {
    const artist = get().artist;
    if (!artist) return undefined;
    const guard = missingGuard(album);
    const token = guard.begin();
    let missing: MissingTracks | null;
    try {
      missing = await getMissingTracks(artist, album);
    } catch (e) {
      console.error("Failed to load missing songs:", e);
      return undefined;
    }
    if (guard.isLatest(token) && get().artist === artist) {
      set((state) => ({ tracks: { ...state.tracks, [album]: missing } }));
      if (missing) {
        // A song that failed and is not missing any more was recovered (another source): forget the failure.
        const recovered = recoveredSongs(get().failureAlbum, missing.source_url, new Set(missing.tracks.map((t) => t.url)));
        if (recovered.length > 0) {
          set((state) => {
            const failures = { ...state.failures };
            const failureAlbum = { ...state.failureAlbum };
            for (const url of recovered) {
              delete failures[url];
              delete failureAlbum[url];
            }
            return { failures, failureAlbum };
          });
        }
      }
    }
    return missing;
  };

  /**
   * Marks the songs as downloading before the download starts (startDownload registers the task synchronously, so
   * the subscription below sees it as busy), then starts it. Callers filter out songs already pending.
   */
  const startTracked = async (missing: MissingTracks, trackUrls: string[]) => {
    set((state) => {
      const pendingUrls = new Map(state.pendingUrls);
      for (const url of trackUrls) pendingUrls.set(url, missing.source_url);
      const failures = { ...state.failures };
      for (const url of trackUrls) delete failures[url];
      return { pendingUrls, failures };
    });
    await useLibraryStore.getState().startDownload(missingDownloadParams(missing, trackUrls));
  };

  return {
    artist: null,
    sources: {},
    shown: new Set(),
    tracks: {},
    pendingUrls: new Map(),
    unavailable: loadUnavailable(),
    failures: {},
    failureAlbum: {},

    loadSources: async (artist) => {
      const token = sourcesGuard.begin();
      if (get().artist !== artist) {
        // Another artist: nothing of the previous one may stay visible.
        set({ artist, sources: {}, shown: new Set(), tracks: {} });
      }
      let list: LibraryAlbumSource[];
      try {
        list = await getArtistAlbumSources(artist);
      } catch (e) {
        console.error("Failed to load library album sources:", e);
        return;
      }
      if (!sourcesGuard.isLatest(token) || get().artist !== artist) return;

      const sources = sourcesByAlbum(list);
      const shown = pruneShown(get().shown, sources);
      set({ sources, shown });
      // A download can add songs to a shown album (and the library refresh calls this again), so re-read them.
      for (const album of shown) void reloadMissing(album);
      // With songs marked as having no stream, the counts need the lists of the other albums too.
      if (get().unavailable.size > 0) {
        for (const [album, source] of Object.entries(sources)) {
          if (source.missing > 0 && !shown.has(album)) void reloadMissing(album);
        }
      }
    },

    toggle: (album) => {
      const shown = toggleShown(get().shown, album);
      set({ shown });
      if (shown.has(album)) void reloadMissing(album);
    },

    unlink: async (artist, album) => {
      try {
        await unlinkAlbumSource(artist, album);
      } catch (e) {
        console.error("Failed to unlink the album:", e);
        return;
      }
      set({
        shown: new Set([...get().shown].filter((a) => a !== album)),
        tracks: { ...get().tracks, [album]: null },
      });
      await get().loadSources(artist);
    },

    show: (album) => {
      set({ shown: new Set(get().shown).add(album) });
      void reloadMissing(album);
    },

    downloadMissing: async (album) => {
      const missing = await reloadMissing(album);
      if (!missing) return;
      const pending = get().pendingUrls;
      const dead = get().unavailable;
      const urls = missingTrackUrls(missing).filter((url) => !pending.has(url) && !dead.has(url));
      if (urls.length === 0) return;
      // Show the songs in the album: they get their download percents there.
      set({ shown: new Set(get().shown).add(album) });
      await startTracked(missing, urls);
    },

    downloadOne: async (album, track) => {
      const missing = await reloadMissing(album);
      if (!missing || !missingTrackUrls(missing).includes(track.url) || get().unavailable.has(track.url)) return;
      // Checked after the await: a second click during the read sees the first one's pending entry.
      if (get().pendingUrls.has(track.url)) return;
      await startTracked(missing, [track.url]);
    },

    retry: async (album, track) => {
      await get().downloadOne(album, track);
    },

    downloadMany: async (album, tracks) => {
      const missing = await reloadMissing(album);
      if (!missing) return;
      const wanted = new Set(tracks.map((t) => t.url));
      const pending = get().pendingUrls;
      const dead = get().unavailable;
      const urls = missingTrackUrls(missing).filter((url) => wanted.has(url) && !pending.has(url) && !dead.has(url));
      if (urls.length === 0) return;
      await startTracked(missing, urls);
    },
  };
});

// A pending song is released when its album has no busy download any more (completed, failed or cancelled).
const handledFailures = new Set<string>();

useLibraryStore.subscribe((state) => {
  const { pendingUrls, unavailable, failures } = useMissingTracksStore.getState();
  if (pendingUrls.size === 0) return;
  const { songs, handledIds } = failedSongs(pendingUrls, state.activeDownloads, handledFailures);
  for (const id of handledIds) handledFailures.add(id);
  if (songs.length > 0) {
    const nextFailures = { ...failures };
    const nextAlbums = { ...useMissingTracksStore.getState().failureAlbum };
    const nextUnavailable = new Set(unavailable);
    for (const song of songs) {
      nextFailures[song.url] = song.message;
      nextAlbums[song.url] = song.album;
      if (song.noStream) nextUnavailable.add(song.url);
    }
    if (nextUnavailable.size !== unavailable.size) saveUnavailable(nextUnavailable);
    useMissingTracksStore.setState({ failures: nextFailures, failureAlbum: nextAlbums, unavailable: nextUnavailable });
  }
  const next = prunePending(useMissingTracksStore.getState().pendingUrls, busyAlbumUrls(state.activeDownloads));
  if (next.size !== pendingUrls.size) useMissingTracksStore.setState({ pendingUrls: next });
});
