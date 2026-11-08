import { create } from "zustand";
import { getArtistAlbums, getArtists } from "@/shared/lib/audio.ts";
import { searchPlatforms } from "@/shared/lib/search.ts";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Album, ArtistSummary, Platform } from "@/shared/types.ts";

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
  itemType: ItemType | "Track" | "Album";
  platform: Platform;
  overallPercent: number;
  tracks: TrackDownloadProgress[];
}

export interface StartDownloadParams {
  platform: Platform | string;
  downloadType: ItemType | string;
  url: string;
  artistName?: string;
  title?: string;
  coverUrl?: string;
}

export type ActiveView =
  | "ARTIST_ALBUMS"
  | "SEARCH_RESULTS"
  | "PLAYLIST"
  | "TRASH";

export enum ItemType {
  Track = "Track",
  Album = "Album",
  Artist = "Artist",
}

export type CategoryFilter = "All" | ItemType;
export type PlatformFilter = "All" | "Youtube" | "Bandcamp";

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
  activeCategory: CategoryFilter;
  activePlatform: PlatformFilter;
  setActiveCategory: (cat: CategoryFilter) => void;
  setActivePlatform: (plat: PlatformFilter) => void;

  fetchTabs: (query?: string) => Promise<void>;
  setSelectedTab: (identifier: string) => void;

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
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  sortType: SortType.Artist,
  displayType: DisplayType.Albums,
  selectedTab: sessionStorage.getItem("selectedTab") || "",
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
        "bandcamp",
        "youtube",
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
    try {
      sessionStorage.setItem("selectedTab", identifier);
    } catch (_) {}
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
          status: string;
          current: number;
          total: number;
          overall_percent: number;
          message: string;
          tracks: {
            title: string;
            track_number?: number;
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
              itemType: (item.download_type === "Track"
                ? ItemType.Track
                : ItemType.Album) as ItemType,
              platform: item.platform as Platform,
              overallPercent: item.overall_percent,
              tracks: (item.tracks || []).map((t) => ({
                title: t.title,
                trackNumber: t.track_number,
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
      tracks: { title: string; track_number?: number }[];
    }>("download-album-meta", (event) => {
      const { task_id, title, artist, cover_url, tracks } = event.payload;
      set((state) => {
        const existing = state.activeDownloads[task_id];
        if (!existing) return {};

        const updatedTracks: TrackDownloadProgress[] = tracks.map((t, idx) => {
          const existingTrack = existing.tracks[idx];
          return {
            title: t.title,
            trackNumber: t.track_number ?? idx + 1,
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
          total: isSingleTrack ? 1 : 0,
          message: "Starting download...",
          artistName,
          albumTitle: isSingleTrack ? "Singles" : title || "Album",
          coverUrl,
          itemType: downloadType as ItemType,
          platform: platform as Platform,
          overallPercent: 0,
          tracks: isSingleTrack
            ? [
                {
                  title: title || "Track",
                  percent: 0,
                  status: "downloading",
                  trackNumber: 1,
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

  cancelDownload: async (taskId: string) => {
    try {
      await invoke("cancel_download", { taskId });
    } catch (e) {
      console.error("Cancel download invoke failed:", e);
    }
    set((state) => {
      const activeDownloads = { ...state.activeDownloads };
      delete activeDownloads[taskId];
      return { activeDownloads };
    });
  },
}));
