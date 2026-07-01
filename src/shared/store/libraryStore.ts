import { create } from "zustand";
import {
  Album,
  ArtistSummary,
  getArtistAlbums,
  getArtists,
} from "@/shared/lib/audio.ts";
import { searchPlatforms } from "@/shared/lib/search.ts";

export type ActiveView = "ARTIST_ALBUMS" | "SEARCH_RESULTS";

export enum ItemType {
  Track = "Track",
  Album = "Album",
  Artist = "Artist",
}

export type SearchSuggestion = {
  item_type: ItemType;
  name: string;
  band_name: string;
  album_name?: string;
  subscriber_count?: number;
  item_url_path: string;
  img: string;
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
  searchQuery: string;
  searchResults: SearchSuggestion[];
  searchCache: Record<string, any[]>;
  setSearchQuery: (query: string) => void;
  executeSearch: (query: string) => Promise<void>;

  setActiveView: (view: ActiveView) => void;

  // setTabs: (result: ArtistSummary[]) => void;
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
  searchQuery: "",
  searchResults: [],
  searchCache: {},

  setActiveView: (view) => set({ activeView: view }),
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

  // setTabs: (result: ArtistSummary[]) => set({ result }),
  // setSortType: (type: SortType) => set({ type }),
  // setDisplayType: (type: DisplayType) => set({ type }),
  setSelectedTab: async (identifier) => {
    set({ selectedTab: identifier, activeView: "ARTIST_ALBUMS" });

    try {
      const albums = await getArtistAlbums(identifier);
      console.log("albums: ", albums);
      set({ albums });
    } catch (error) {
      console.error("Failed to fetch artist albums:", error);
      set({ albums: [] });
    }
  },
  // setAlbums: (albums: Album[]) => set({ albums }),

  fetchTabs: async (query?: string): Promise<void> => {
    const { sortType } = get();
    let results: ArtistSummary[] = [];

    if (sortType === SortType.Artist) {
      results = await getArtists(query);
    }

    set({ tabs: results });
  },
}));
