import { create } from "zustand";
import {
  Album,
  ArtistSummary,
  getArtistAlbums,
  getArtists,
} from "@/shared/lib/audio.ts";
import { Platform, searchPlatforms } from "@/shared/lib/search.ts";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export type DownloadStatus =
  | "idle"
  | "downloading"
  | "processing"
  | "completed"
  | "failed";

export interface DownloadProgress {
  id: string;
  url: string;
  status: DownloadStatus;
  current: number;
  total: number;
  message: string;
}

export type ActiveView = "ARTIST_ALBUMS" | "SEARCH_RESULTS";

export enum ItemType {
  Track = "Track",
  Album = "Album",
  Artist = "Artist",
}

export type FilterCategory = "All" | ItemType;
export type PlatformFilter = "All" | "Youtube" | "Bandcamp" | "Local";

export type SearchSuggestion = {
  item_type: ItemType;
  name: string;
  band_name: string;
  album_name?: string;
  item_url_path: string;
  img: string;
  subscriber_count?: number;
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

  // Search Bar Live State
  searchQuery: string;
  searchResults: SearchSuggestion[];
  searchCache: Record<string, any[]>;
  setSearchQuery: (query: string) => void;
  executeSearch: (query: string) => Promise<void>;

  // Main Window Committed State
  submittedQuery: string;
  committedResults: SearchSuggestion[];
  commitSearch: (query: string) => void;

  // Global Filters
  activeCategory: FilterCategory;
  activePlatform: PlatformFilter;
  setActiveCategory: (cat: FilterCategory) => void;
  setActivePlatform: (plat: PlatformFilter) => void;

  fetchTabs: (query?: string) => Promise<void>;
  setSelectedTab: (identifier: string) => void;

  // Download tracking
  activeDownloads: Record<string, DownloadProgress>;
  _listenersInitialized: boolean;
  initDownloadListeners: () => Promise<void>;
  startDownload: (
    platform: string,
    downloadType: string,
    url: string,
  ) => Promise<void>;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  sortType: SortType.Artist,
  displayType: DisplayType.Albums,
  selectedTab: "",
  tabs: [],
  albums: [],

  activeView: "ARTIST_ALBUMS",
  setActiveView: (view) => set({ activeView: view }),

  searchQuery: "",
  searchResults: [],
  searchCache: {},

  submittedQuery: "",
  committedResults: [],

  activeCategory: "All",
  activePlatform: "All",
  setActiveCategory: (cat) => set({ activeCategory: cat }),
  setActivePlatform: (plat) => set({ activePlatform: plat }),

  setSearchQuery: (query) => set({ searchQuery: query }),
  executeSearch: async (query) => {
    const trimmedQuery = query.trim().toLowerCase();
    if (!trimmedQuery) {
      set({ searchResults: [] });
      return;
    }

    const { searchCache } = get();

    if (searchCache[trimmedQuery]) {
      set({ searchResults: searchCache[trimmedQuery] });
      return;
    }

    try {
      const results: SearchSuggestion[] = await searchPlatforms(trimmedQuery, [
        "Bandcamp",
        "Youtube",
      ]);

      set((state) => ({
        searchResults: results,
        searchCache: {
          ...state.searchCache,
          [trimmedQuery]: results,
        },
      }));
    } catch (error) {
      console.error("Search failed", error);
    }
  },

  commitSearch: (query) => {
    set((state) => ({
      submittedQuery: query,
      committedResults: state.searchResults,
      activeView: "SEARCH_RESULTS",
    }));
  },

  setSelectedTab: async (identifier) => {
    set({ selectedTab: identifier, activeView: "ARTIST_ALBUMS" });
    try {
      const albums = await getArtistAlbums(identifier);
      set({ albums });
    } catch (error) {
      console.error("Failed to fetch artist albums:", error);
      set({ albums: [] });
    }
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

    await listen<{ task_id: string; message: string }>(
      "download-task-started",
      (event) => {
        set((state) => ({
          activeDownloads: {
            ...state.activeDownloads,
            [event.payload.task_id]: {
              ...state.activeDownloads[event.payload.task_id],
              id: event.payload.task_id,
              status: "downloading",
              message: event.payload.message,
            },
          },
        }));
      },
    );

    await listen<{
      task_id: string;
      current: number;
      total: number;
      track_title: string;
    }>("download-task-progress", (event) => {
      const { task_id, current, total, track_title } = event.payload;
      console.log("download-task-progress", event.payload);
      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};
        return {
          activeDownloads: {
            ...state.activeDownloads,
            [task_id]: {
              ...existing,
              status: "downloading",
              current,
              total,
              message: track_title,
            },
          },
        };
      });
    });

    await listen<{ task_id: string; message: string }>(
      "download-task-completed",
      (event) => {
        set((state) => {
          const existing = state.activeDownloads[event.payload.task_id];
          if (!existing) return {};
          return {
            activeDownloads: {
              ...state.activeDownloads,
              [event.payload.task_id]: {
                ...existing,
                status: "completed",
                message: event.payload.message,
              },
            },
          };
        });

        console.log(event.payload.message, ": ", event.payload.task_id);
        console.log(get().activeDownloads);

        get()
          .fetchTabs()
          .catch((e) =>
            console.error("Auto-fetch failed", event.payload.task_id, e),
          );
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
  },

  startDownload: async (platform, downloadType, url) => {
    const taskId = crypto.randomUUID();

    set((state) => ({
      activeDownloads: {
        ...state.activeDownloads,
        [taskId]: {
          id: taskId,
          url,
          status: "idle",
          current: 0,
          total: 0,
          message: "Queueing...",
        },
      },
    }));

    try {
      await invoke("download", {
        id: taskId,
        platform,
        downloadType,
        url,
      });
    } catch (e) {
      console.error("Download invoke failed:", e);
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
}));
