import { create } from "zustand";
import { getArtistAlbums, getArtists } from "@/shared/lib/audio.ts";
import { searchPlatforms } from "@/shared/lib/search/search.ts";
import { type PlatformFilter, SEARCH_PLATFORMS } from "@/shared/lib/remote/platforms.ts";
import { createLatestGuard } from "@/shared/lib/search/searchGuard.ts";
import {
  applyProgress,
  closeProgress,
  type SearchRun,
  type SearchSource,
  type SourceProgressEvent,
} from "@/shared/lib/search/searchProgress.ts";
import { listen } from "@tauri-apps/api/event";
import { Album, ArtistSummary, Platform } from "@/shared/types.ts";
import { captureFrontendError } from "@/shared/lib/telemetry.ts";
import { loadSavedNavEntry } from "@/features/player/model/playerPersistence.ts";

export type { DownloadStatus, DownloadProgress, DownloadOverrides, StartDownloadParams, TrackPlacement } from "@/shared/store/downloadStore.ts";

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
  searchCacheExpires: Record<string, number>;
  searchWarning: string | null;
  /** The query whose first results are shown while slower sources are still searching. */
  partialResultsFor: string | null;
  /** What the current or last dropdown search asked for and how its sources did. */
  searchRun: SearchRun | null;
  setSearchQuery: (query: string) => void;
  executeSearch: (query: string, force?: boolean, sources?: SearchSource[]) => Promise<void>;
  retrySearchSource: (source: SearchSource) => Promise<void>;

  // Global Filters
  activeCategory: CategoryFilter;
  activePlatform: PlatformFilter;
  setActiveCategory: (cat: CategoryFilter) => void;
  setActivePlatform: (plat: PlatformFilter) => void;

  fetchTabs: (query?: string) => Promise<void>;
  setSelectedTab: (identifier: string) => void;
  cancelTabLoad: () => void;
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
  searchCacheExpires: {},
  searchWarning: null,
  partialResultsFor: null,
  searchRun: null,

  activeCategory: "All",
  activePlatform: "All",
  setActiveCategory: (cat) => set({ activeCategory: cat }),
  setActivePlatform: (plat) => set({ activePlatform: plat }),

  setSearchQuery: (query) => set({ searchQuery: query }),
  retrySearchSource: async (source) => {
    const run = get().searchRun;
    if (!run || run.finishedAt === null || !run.sources.some((s) => s.source === source && (s.state === "failed" || s.state === "unknown"))) return;
    await get().executeSearch(run.query, true, [source]);
  },
  executeSearch: async (query, force = false, sources) => {
    const previousRun = get().searchRun;
    const previousResults = sources ? get().searchResults : [];
    const mergeResults = (items: SearchSuggestion[]) => sources
      ? [...new Map([...previousResults, ...items].map((item) => [item.item_url_path, item])).values()]
      : items;
    const token = searchGuard.begin();
    const trimmedQuery = query.trim().toLowerCase();
    if (!trimmedQuery) {
      set({ searchResults: [], searchWarning: null, partialResultsFor: null, searchRun: null });
      return;
    }

    set({ searchWarning: null, partialResultsFor: null });
    const { searchCache, searchCacheExpires } = get();

    const run: SearchRun = {
      query: query.trim(),
      startedAt: Date.now(),
      finishedAt: null,
      cached: false,
      sources: sources ? (previousRun?.sources ?? []).map((entry) => sources.includes(entry.source) ? { ...entry, state: "waiting", elapsedMs: null, count: null, startedAt: null } : entry) : [],
    };
    if (!force && searchCache[trimmedQuery] && searchCacheExpires[trimmedQuery] > Date.now()) {
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
            if (finished) return;
            if (ev.payload.requestId !== token) return;
            run.sources = applyProgress(run.sources, ev.payload.progress, Date.now());
            if (searchGuard.isLatest(token)) set({ searchRun: { ...run } });
          },
        ));
        unlisten.push(await listen<{ requestId: number; results: SearchSuggestion[] }>(
          "search-partial",
          (ev) => {
            // An event that arrives late must not replace the complete results.
            if (finished || !searchGuard.isLatest(token)) return;
            if (ev.payload.requestId !== token) return;
            set({ searchResults: mergeResults(ev.payload.results), partialResultsFor: trimmedQuery });
          },
        ));
      }
      const found: SearchSuggestion[] = await searchPlatforms(
        trimmedQuery,
        SEARCH_PLATFORMS,
        token,
        ...(sources ? [sources] as [SearchSource[]] : []),
      );
      const results = mergeResults(found);
      finished = true;
      const isLatest = searchGuard.isLatest(token);

      set((state) => {
        const nextCache = { ...state.searchCache };
        const nextExpires = { ...state.searchCacheExpires };
        const now = Date.now();
        for (const key of Object.keys(nextCache)) {
          if (!(nextExpires[key] > now)) {
            delete nextCache[key];
            delete nextExpires[key];
          }
        }
        const complete = !warning && run.sources.every((source) => source.state === "done");
        if (complete) {
          nextCache[trimmedQuery] = results;
          nextExpires[trimmedQuery] = now + 5 * 60 * 1000;
        }
        return {
          ...(isLatest ? { searchResults: results, searchWarning: warning, partialResultsFor: null } : {}),
          searchCache: nextCache,
          searchCacheExpires: nextExpires,
        };
      });
    } catch (error) {
      console.error("Search failed", error);
      captureFrontendError(error, "libraryStore.executeSearch");
    } finally {
      finished = true;
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

}));
