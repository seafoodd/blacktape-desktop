import { create } from "zustand";
import { getArtistAlbums, getArtists } from "@/shared/lib/audio.ts";
import { searchPlatforms } from "@/shared/lib/search/search.ts";
import { type PlatformFilter, SEARCH_PLATFORMS } from "@/shared/lib/remote/platforms.ts";
import { createLatestGuard } from "@/shared/lib/search/searchGuard.ts";
import {
  applyProgress,
  closeProgress,
  type SearchRun,
  type SourceProgressEvent,
} from "@/shared/lib/search/searchProgress.ts";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Album, ArtistSummary, Platform } from "@/shared/types.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import { findDownload, isRunning } from "@/shared/lib/library/downloadMatch.ts";
import {
  captureFrontendError,
  isCancellationError,
} from "@/shared/lib/telemetry.ts";
import { loadSavedNavEntry } from "@/features/player/model/playerPersistence.ts";

export type DownloadStatus =
  | "idle"
  | "downloading"
  | "processing"
  | "completed"
  | "failed"
  | "cancelled";

export interface TrackDownloadProgress {
  title: string;
  percent: number;
  status: "pending" | "downloading" | "completed" | "failed";
  trackNumber?: number;
  discNumber?: number;
}

export interface DownloadProgress {
  id: string;
  url: string;
  status: DownloadStatus;
  current: number;
  total: number;
  message: string;
  artistName: string;
  albumTitle: string;
  coverUrl?: string;
  releaseYear?: number;
  itemType: ItemType | "Track" | "Album";
  platform: Platform;
  overallPercent: number;
  tracks: TrackDownloadProgress[];
  /** A single track filed into a library song: its progress belongs to that song, whatever title the platform gave it. */
  placement?: TrackPlacement;
  /** Album downloads of some tracks: their page urls (absent for a whole album and for recovered downloads). */
  trackUrls?: string[];
  /** Library song id when upgrading/replacing a single existing track. */
  replaceSongId?: number;
}

interface TrackOverride {
  index?: number;
  original_url?: string;
  title?: string;
  url?: string;
  fallback_urls?: string[];
  candidates?: AudioCandidate[];
  selected_candidate_idx?: number;
}

export interface DownloadOverrides {
  title?: string;
  artist?: string;
  year?: number | null;
  tracks?: TrackOverride[];
}

export interface StartDownloadParams {
  platform: Platform | string;
  downloadType: ItemType | string;
  url: string;
  artistName?: string;
  title?: string;
  coverUrl?: string;
  /** Album downloads only: page urls of the tracks to download (all tracks when omitted). */
  trackUrls?: string[];
  /** Single track downloads: the library album and song this track stands for (names and number win over the platform's). */
  placement?: TrackPlacement;
  /** yt-dlp format id picked in the quality menu (the platform's best is the fallback); omitted: global default. */
  format?: string;
  /** Edited album/track metadata & chosen candidates staged in AlbumPreview. */
  overrides?: DownloadOverrides;
  /** Library song id to atomically upgrade and replace (old file moves to Trash after verified download). */
  replaceSongId?: number;
}

/** Mirrors the Rust `TrackPlacement` (snake_case on the wire). */
export interface TrackPlacement {
  album: string;
  album_artist: string;
  title: string;
  track_number: number | null;
}

export type ActiveView =
  | "ARTIST_ALBUMS"
  | "SEARCH_RESULTS"
  | "ALBUM_PREVIEW"
  | "PLAYLIST"
  | "TRASH"
  | "FAVORITES";

export enum ItemType {
  Track = "Track",
  Album = "Album",
  Artist = "Artist",
}

export type CategoryFilter = "All" | ItemType;
export type { PlatformFilter };

export type SearchSuggestion = {
  item_type: ItemType;
  name: string;
  band_name: string;
  album_name?: string;
  item_url_path: string;
  /** Page url of the album a track belongs to (YouTube Music); lets the UI open the album from the track. */
  album_url?: string | null;
  img: string;
  subscriber_count?: number;
  /** What `subscriber_count` counts (YouTube artist rows): followers or monthly listeners. */
  audience_kind?: "subscribers" | "monthly_audience" | "followers" | null;
  view_count?: number;
  year?: number;
  duration?: number;
  platform: Platform;
};

enum SortType {
  Artist,
}

enum DisplayType {
  Songs,
  Albums,
}

interface LibraryState {
  sortType: SortType;
  displayType: DisplayType;
  selectedTab: string;
  tabs: ArtistSummary[];
  albums: Album[];

  activeView: ActiveView;
  setActiveView: (view: ActiveView) => void;
  scrollToAlbum: string | null;
  setScrollToAlbum: (albumTitle: string | null) => void;

  // Search Bar Live State
  searchQuery: string;
  searchResults: SearchSuggestion[];
  searchCache: Record<string, any[]>;
  searchWarning: string | null;
  /** The query whose first results are shown while slower sources are still searching. */
  partialResultsFor: string | null;
  /** What the current or last dropdown search asked for and how its sources did. */
  searchRun: SearchRun | null;
  setSearchQuery: (query: string) => void;
  executeSearch: (query: string, force?: boolean) => Promise<void>;

  // Global Filters
  activeCategory: CategoryFilter;
  activePlatform: PlatformFilter;
  setActiveCategory: (cat: CategoryFilter) => void;
  setActivePlatform: (plat: PlatformFilter) => void;

  fetchTabs: (query?: string) => Promise<void>;
  setSelectedTab: (identifier: string) => void;
  cancelTabLoad: () => void;

  // Download tracking
  activeDownloads: Record<string, DownloadProgress>;
  _listenersInitialized: boolean;
  initDownloadListeners: () => Promise<void>;
  startDownload: (
    paramsOrPlatform: StartDownloadParams | string,
    downloadType?: string,
    url?: string,
  ) => Promise<void>;
  cancelDownload: (taskId: string) => Promise<void>;
  dismissDownload: (taskId: string) => void;
}

// Only the most recently started search or artist-tab load may commit results.
const searchGuard = createLatestGuard();
const tabGuard = createLatestGuard();

const savedInitialNav =
  typeof window !== "undefined" ? loadSavedNavEntry() : null;

export const useLibraryStore = create<LibraryState>((set, get) => ({
  sortType: SortType.Artist,
  displayType: DisplayType.Albums,
  selectedTab: savedInitialNav?.artistName || "",
  tabs: [],
  albums: [],

  activeView: savedInitialNav?.view || "ARTIST_ALBUMS",
  setActiveView: (view) => set({ activeView: view }),
  scrollToAlbum: null,
  setScrollToAlbum: (albumTitle) => set({ scrollToAlbum: albumTitle }),

  searchQuery: "",
  searchResults: [],
  searchCache: {},
  searchWarning: null,
  partialResultsFor: null,
  searchRun: null,

  activeCategory: "All",
  activePlatform: "All",
  setActiveCategory: (cat) => set({ activeCategory: cat }),
  setActivePlatform: (plat) => set({ activePlatform: plat }),

  setSearchQuery: (query) => set({ searchQuery: query }),
  executeSearch: async (query, force = false) => {
    const token = searchGuard.begin();
    const trimmedQuery = query.trim().toLowerCase();
    if (!trimmedQuery) {
      set({ searchResults: [], searchWarning: null, partialResultsFor: null, searchRun: null });
      return;
    }

    set({ searchWarning: null, partialResultsFor: null });
    const { searchCache } = get();

    const run: SearchRun = {
      query: query.trim(),
      startedAt: Date.now(),
      finishedAt: null,
      cached: false,
      sources: [],
    };
    if (!force && searchCache[trimmedQuery]) {
      // The same query again, such as after a typed space, keeps the timings of its search.
      const previous = get().searchRun;
      const searchedJustNow =
        previous !== null &&
        previous.finishedAt !== null &&
        previous.query.toLowerCase() === trimmedQuery;
      set({
        searchResults: searchCache[trimmedQuery],
        searchRun: searchedJustNow ? previous : { ...run, finishedAt: run.startedAt, cached: true },
      });
      return;
    }
    set({ searchRun: run });

    let warning: string | null = null;
    let finished = false;
    const unlisten: (() => void)[] = [];
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        unlisten.push(await listen<{ requestId: number; message: string }>(
          "search-mb-warning",
          (ev) => {
            if (ev.payload.requestId === token) {
              warning = ev.payload.message;
            }
          },
        ));
        unlisten.push(await listen<{ requestId: number; progress: SourceProgressEvent }>(
          "search-progress",
          (ev) => {
            if (finished || !searchGuard.isLatest(token)) return;
            if (ev.payload.requestId !== token) return;
            run.sources = applyProgress(run.sources, ev.payload.progress, Date.now());
            set({ searchRun: { ...run } });
          },
        ));
        unlisten.push(await listen<{ requestId: number; results: SearchSuggestion[] }>(
          "search-partial",
          (ev) => {
            // An event that arrives late must not replace the complete results.
            if (finished || !searchGuard.isLatest(token)) return;
            if (ev.payload.requestId !== token) return;
            set({ searchResults: ev.payload.results, partialResultsFor: trimmedQuery });
          },
        ));
      }
      const results: SearchSuggestion[] = await searchPlatforms(
        trimmedQuery,
        SEARCH_PLATFORMS,
        token,
      );
      finished = true;
      const isLatest = searchGuard.isLatest(token);

      set((state) => {
        const nextCache = { ...state.searchCache };
        // Don't poison cache if MusicBrainz was unavailable (503) so subsequent search retries cleanly.
        if (!warning) {
          nextCache[trimmedQuery] = results;
        }
        return {
          ...(isLatest ? { searchResults: results, searchWarning: warning } : {}),
          searchCache: nextCache,
        };
      });
    } catch (error) {
      console.error("Search failed", error);
      captureFrontendError(error, "libraryStore.executeSearch");
    } finally {
      unlisten.forEach((stop) => stop());
      if (searchGuard.isLatest(token)) {
        set({ searchRun: { ...run, finishedAt: Date.now(), sources: closeProgress(run.sources) } });
      }
    }
  },

  setSelectedTab: async (identifier) => {
    const token = tabGuard.begin();
    set({ selectedTab: identifier, activeView: "ARTIST_ALBUMS" });
    try {
      const albums = await getArtistAlbums(identifier);
      if (!tabGuard.isLatest(token)) return;
      set({ albums });
    } catch (error) {
      if (!tabGuard.isLatest(token)) return;
      console.error("Failed to fetch artist albums:", error);
      captureFrontendError(error, "libraryStore.getArtistAlbums");
      set({ albums: [] });
    }
  },

  cancelTabLoad: () => {
    tabGuard.begin();
  },

  fetchTabs: async (query?: string): Promise<void> => {
    const { sortType } = get();
    let results: ArtistSummary[] = [];
    if (sortType === SortType.Artist) {
      results = await getArtists(query);
    }

    set({ tabs: results });
  },

  activeDownloads: {},
  _listenersInitialized: false,

  initDownloadListeners: async () => {
    if (get()._listenersInitialized) return;
    set({ _listenersInitialized: true });

    try {
      const activeList = await invoke<
        {
          id: string;
          platform: string;
          download_type: string;
          url: string;
          artist_name: string;
          album_title: string;
          cover_url?: string;
          release_year?: number;
          status: string;
          current: number;
          total: number;
          overall_percent: number;
          message: string;
          tracks: {
            title: string;
            track_number?: number;
            disc_number?: number;
            percent: number;
            status: string;
          }[];
        }[]
      >("get_active_downloads");

      if (activeList && activeList.length > 0) {
        set((state) => {
          const activeDownloads = { ...state.activeDownloads };
          for (const item of activeList) {
            activeDownloads[item.id] = {
              id: item.id,
              url: item.url,
              status: item.status as DownloadStatus,
              current: item.current,
              total: item.total,
              message: item.message,
              artistName: item.artist_name,
              albumTitle: item.album_title,
              coverUrl: item.cover_url,
              releaseYear: item.release_year,
              itemType: (item.download_type === "Track"
                ? ItemType.Track
                : ItemType.Album) as ItemType,
              platform: item.platform as Platform,
              overallPercent: item.overall_percent,
              tracks: (item.tracks || []).map((t) => ({
                title: t.title,
                trackNumber: t.track_number,
                discNumber: t.disc_number,
                percent: t.percent,
                status: t.status as any,
              })),
            };
          }
          return { activeDownloads };
        });
      }
    } catch (e) {
      console.error("Failed to recover active downloads on load:", e);
    }

    await listen<{ task_id: string; message: string }>(
      "download-task-started",
      (event) => {
        set((state) => {
          const existing = state.activeDownloads[event.payload.task_id];
          if (!existing) return {};
          return {
            activeDownloads: {
              ...state.activeDownloads,
              [event.payload.task_id]: {
                ...existing,
                status: "downloading",
                message: event.payload.message,
              },
            },
          };
        });
      },
    );

    await listen<{
      task_id: string;
      title: string;
      artist: string;
      cover_url?: string;
      release_year?: number;
      tracks: { title: string; track_number?: number; disc_number?: number }[];
    }>("download-album-meta", (event) => {
      const { task_id, title, artist, cover_url, release_year, tracks } = event.payload;
      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};

        const updatedTracks: TrackDownloadProgress[] = tracks.map((t, idx) => {
          const existingTrack = existing.tracks[idx];
          return {
            title: t.title,
            trackNumber: t.track_number ?? idx + 1,
            discNumber: t.disc_number,
            percent: existingTrack ? existingTrack.percent : 0,
            status: existingTrack ? existingTrack.status : "pending",
          };
        });

        return {
          activeDownloads: {
            ...state.activeDownloads,
            [task_id]: {
              ...existing,
              artistName: artist || existing.artistName,
              albumTitle: title || existing.albumTitle,
              coverUrl: cover_url || existing.coverUrl,
              releaseYear: release_year ?? existing.releaseYear,
              total: tracks.length,
              tracks: updatedTracks,
            },
          },
        };
      });
    });

    await listen<{
      task_id: string;
      track_index: number;
      track_title: string;
      percent: number;
    }>("download-track-progress", (event) => {
      const { task_id, track_index, track_title, percent } = event.payload;
      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};

        const tracks = [...existing.tracks];
        while (tracks.length <= track_index) {
          tracks.push({
            title: track_title || `Track ${tracks.length + 1}`,
            percent: 0,
            status: "pending",
            trackNumber: tracks.length + 1,
          });
        }

        const isComplete = percent >= 100;
        tracks[track_index] = {
          ...tracks[track_index],
          title: track_title || tracks[track_index].title,
          percent: Math.min(100, Math.max(0, percent)),
          status: isComplete ? "completed" : "downloading",
        };

        const totalTracks = tracks.length;
        const totalPercentSum = tracks.reduce((sum, t) => sum + t.percent, 0);
        const overallPercent =
          totalTracks > 0 ? Math.round(totalPercentSum / totalTracks) : percent;

        return {
          activeDownloads: {
            ...state.activeDownloads,
            [task_id]: {
              ...existing,
              tracks,
              overallPercent,
            },
          },
        };
      });
    });

    let trackSavedDebounceTimer: ReturnType<typeof setTimeout> | null = null;
    await listen<{
      task_id: string;
      track_index: number;
      song: any;
    }>("download-track-saved", (event) => {
      const { task_id, track_index, song } = event.payload;

      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};

        const tracks = [...existing.tracks];
        while (tracks.length <= track_index) {
          tracks.push({
            title: song?.title || `Track ${tracks.length + 1}`,
            percent: 0,
            status: "pending",
            trackNumber: tracks.length + 1,
          });
        }

        tracks[track_index] = {
          ...tracks[track_index],
          title: song?.title || tracks[track_index].title,
          percent: 100,
          status: "completed",
        };

        const totalTracks = tracks.length;
        const totalPercentSum = tracks.reduce((sum, t) => sum + t.percent, 0);
        const overallPercent =
          totalTracks > 0 ? Math.round(totalPercentSum / totalTracks) : 100;

        return {
          activeDownloads: {
            ...state.activeDownloads,
            [task_id]: {
              ...existing,
              tracks,
              overallPercent,
            },
          },
        };
      });

      if (song?.source_url) {
        invoke("swap_streams_for_downloads", { songs: [song] }).catch((e) =>
          console.error("swap_streams_for_downloads per-track error:", e),
        );
      }

      if (trackSavedDebounceTimer) {
        clearTimeout(trackSavedDebounceTimer);
      }
      trackSavedDebounceTimer = setTimeout(async () => {
        trackSavedDebounceTimer = null;
        get()
          .fetchTabs()
          .catch((e) => console.error("Auto-fetch tabs on track saved failed", e));

        const { selectedTab } = get();
        if (selectedTab) {
          try {
            const albums = await getArtistAlbums(selectedTab);
            set({ albums });
          } catch (e) {
            console.error("Auto-fetch albums on track saved failed", e);
          }
        }
      }, 150);
    });

    await listen<{
      task_id: string;
      current: number;
      total: number;
      track_title: string;
    }>("download-task-progress", (event) => {
      const { task_id, current, total, track_title } = event.payload;
      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};
        const overallPercent =
          existing.tracks.length > 0
            ? existing.overallPercent
            : total > 0
              ? Math.round((current / total) * 100)
              : existing.overallPercent;

        return {
          activeDownloads: {
            ...state.activeDownloads,
            [task_id]: {
              ...existing,
              status: "downloading",
              current,
              total: total > 0 ? total : existing.total,
              message: track_title,
              overallPercent,
            },
          },
        };
      });
    });

    await listen<{ task_id: string; message: string }>(
      "download-task-completed",
      async (event) => {
        const taskId = event.payload.task_id;
        const downloadedArtist = get().activeDownloads[taskId]?.artistName;
        set((state) => {
          const existing = state.activeDownloads[taskId];
          if (!existing) return {};
          const tracks = existing.tracks.map((t) => ({
            ...t,
            percent: 100,
            status: "completed" as const,
          }));
          return {
            activeDownloads: {
              ...state.activeDownloads,
              [taskId]: {
                ...existing,
                status: "completed",
                overallPercent: 100,
                tracks,
                message: event.payload.message,
              },
            },
          };
        });

        get()
          .fetchTabs()
          .catch((e) => console.error("Auto-fetch failed", taskId, e));

        const { selectedTab } = get();
        if (selectedTab) {
          try {
            const albums = await getArtistAlbums(selectedTab);
            set({ albums });
          } catch (e) {
            console.error("Auto-fetch albums failed", taskId, e);
          }
        }

        // Streams of the songs that were just downloaded (playing or queued) switch to the files.
        if (downloadedArtist) {
          try {
            const songs = (await getArtistAlbums(downloadedArtist))
              .flatMap((album) => album.songs)
              .filter((song) => !!song.source_url);
            if (songs.length > 0) await invoke("swap_streams_for_downloads", { songs });
          } catch (e) {
            console.error("swap_streams_for_downloads failed", taskId, e);
          }
        }

        setTimeout(() => {
          set((state) => {
            const active = { ...state.activeDownloads };
            if (active[taskId]?.status === "completed") {
              delete active[taskId];
              return { activeDownloads: active };
            }
            return {};
          });
        }, 2500);
      },
    );

    await listen<{ task_id: string; message: string }>(
      "download-task-failed",
      (event) => {
        set((state) => {
          const existing = state.activeDownloads[event.payload.task_id];
          if (!existing) return {};
          return {
            activeDownloads: {
              ...state.activeDownloads,
              [event.payload.task_id]: {
                ...existing,
                status: "failed",
                message: event.payload.message,
              },
            },
          };
        });
      },
    );

    await listen<{ task_id: string }>("download-task-cancelled", (event) => {
      set((state) => {
        const activeDownloads = { ...state.activeDownloads };
        delete activeDownloads[event.payload.task_id];
        return { activeDownloads };
      });
    });
  },

  startDownload: async (paramsOrPlatform, downloadTypeArg, urlArg) => {
    const isObject = typeof paramsOrPlatform === "object";
    const platform = isObject ? paramsOrPlatform.platform : paramsOrPlatform;
    const downloadType = isObject
      ? paramsOrPlatform.downloadType
      : downloadTypeArg!;
    const url = isObject ? paramsOrPlatform.url : urlArg!;
    const artistName = isObject
      ? paramsOrPlatform.artistName || "Unknown Artist"
      : "Unknown Artist";
    const title = isObject ? paramsOrPlatform.title || "" : "";
    const coverUrl = isObject ? paramsOrPlatform.coverUrl : undefined;
    const trackUrls = isObject ? paramsOrPlatform.trackUrls : undefined;
    const placement = isObject ? paramsOrPlatform.placement : undefined;
    const format = isObject ? paramsOrPlatform.format : undefined;
    const overrides = isObject ? paramsOrPlatform.overrides : undefined;
    const replaceSongId = isObject ? paramsOrPlatform.replaceSongId : undefined;

    // A second click while the same songs are still downloading would queue them again.
    if (isRunning(findDownload(get().activeDownloads, { url, trackUrls }))) return;

    const taskId = crypto.randomUUID();
    const isSingleTrack =
      downloadType === ItemType.Track || downloadType === "Track";

    set((state) => ({
      activeDownloads: {
        ...state.activeDownloads,
        [taskId]: {
          id: taskId,
          url,
          status: "downloading",
          current: 0,
          total: isSingleTrack ? 1 : (trackUrls?.length ?? 0),
          message: "Starting download...",
          artistName,
          // A single track has no album title until its page was read (then "Singles" or its album).
          albumTitle: placement ? placement.album : isSingleTrack ? "" : title || "Album",
          coverUrl,
          itemType: downloadType as ItemType,
          platform: platform as Platform,
          overallPercent: 0,
          placement,
          trackUrls,
          replaceSongId,
          tracks: isSingleTrack
            ? [
                {
                  title: title || "Track",
                  percent: 0,
                  status: "downloading",
                  trackNumber: placement?.track_number ?? 1,
                },
              ]
            : [],
        },
      },
    }));

    try {
      await invoke("download", {
        id: taskId,
        platform,
        downloadType,
        url,
        artistName,
        title,
        coverUrl,
        trackUrls: trackUrls ?? null,
        placement: placement ?? null,
        format: format ?? null,
        overrides: overrides ?? null,
        replaceSongId: replaceSongId ?? null,
      });
    } catch (e) {
      console.error("Download invoke failed:", e);
      if (!isCancellationError(e)) {
        captureFrontendError(e, "libraryStore.startDownload");
      }
      set((state) => {
        const existing = state.activeDownloads[taskId];
        if (!existing) return {};
        return {
          activeDownloads: {
            ...state.activeDownloads,
            [taskId]: {
              ...existing,
              status: "failed",
              message: String(e),
            },
          },
        };
      });
    }
  },

  cancelDownload: async (taskId: string) => {
    try {
      await invoke("cancel_download", { taskId });
    } catch (e) {
      console.error("Cancel download invoke failed:", e);
      captureFrontendError(e, "libraryStore.cancelDownload");
    }
    set((state) => {
      const activeDownloads = { ...state.activeDownloads };
      delete activeDownloads[taskId];
      return { activeDownloads };
    });
  },

  dismissDownload: (taskId: string) => {
    set((state) => {
      if (!state.activeDownloads[taskId]) return {};
      const activeDownloads = { ...state.activeDownloads };
      delete activeDownloads[taskId];
      return { activeDownloads };
    });
  },
}));
