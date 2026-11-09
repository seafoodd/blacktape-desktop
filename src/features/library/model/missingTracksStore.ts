import { create } from "zustand";
import {
  getArtistAlbumSources,
  getMissingTracks,
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
  /** Loads the sources of `artist`; reloads the missing songs of the albums that are shown. */
  loadSources: (artist: string) => Promise<void>;
  /** Shows or hides the missing songs of `album` (loads them when shown). */
  toggle: (album: string) => void;
  /** Starts one album download of the missing songs of `album` that are not already downloading. */
  downloadMissing: (album: string) => Promise<void>;
  /** Starts the album download of one missing song of `album`. Ignored when it is no longer missing or is downloading. */
  downloadOne: (album: string, track: AlbumTrackInfo) => Promise<void>;
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
      return { pendingUrls };
    });
    await useLibraryStore.getState().startDownload(missingDownloadParams(missing, trackUrls));
  };

  return {
    artist: null,
    sources: {},
    shown: new Set(),
    tracks: {},
    pendingUrls: new Map(),

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
    },

    toggle: (album) => {
      const shown = toggleShown(get().shown, album);
      set({ shown });
      if (shown.has(album)) void reloadMissing(album);
    },

    downloadMissing: async (album) => {
      const missing = await reloadMissing(album);
      if (!missing) return;
      const pending = get().pendingUrls;
      const urls = missingTrackUrls(missing).filter((url) => !pending.has(url));
      if (urls.length === 0) return;
      await startTracked(missing, urls);
    },

    downloadOne: async (album, track) => {
      const missing = await reloadMissing(album);
      if (!missing || !missingTrackUrls(missing).includes(track.url)) return;
      // Checked after the await: a second click during the read sees the first one's pending entry.
      if (get().pendingUrls.has(track.url)) return;
      await startTracked(missing, [track.url]);
    },
  };
});

// A pending song is released when its album has no busy download any more (completed, failed or cancelled).
useLibraryStore.subscribe((state) => {
  const { pendingUrls } = useMissingTracksStore.getState();
  if (pendingUrls.size === 0) return;
  const next = prunePending(pendingUrls, busyAlbumUrls(state.activeDownloads));
  if (next.size !== pendingUrls.size) useMissingTracksStore.setState({ pendingUrls: next });
});
