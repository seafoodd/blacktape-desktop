import { create } from "zustand";
import {
  Album,
  ArtistSummary,
  getArtistAlbums,
  getArtists,
} from "@/shared/lib/audio.ts";
import { Platform, searchPlatforms } from "@/shared/lib/search.ts";

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
    // Lock the current live results into the main window view
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
}));
