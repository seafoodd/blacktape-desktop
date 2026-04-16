import { create } from "zustand";
import {
  Album,
  ArtistSummary,
  getArtistAlbums,
  getArtists,
} from "@/shared/lib/audio.ts";

interface LibraryState {
  sortType: SortType;
  displayType: DisplayType;
  selectedTab: string;
  tabs: ArtistSummary[];
  albums: Album[];

  // setTabs: (result: ArtistSummary[]) => void;
  fetchTabs: (query?: string) => Promise<void>;
  setSelectedTab: (identifier: string) => void;
}

enum SortType {
  Artist,
}

enum DisplayType {
  Songs,
  Albums,
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  sortType: SortType.Artist,
  displayType: DisplayType.Albums,
  selectedTab: "",
  tabs: [],
  albums: [],

  // setTabs: (result: ArtistSummary[]) => set({ result }),
  // setSortType: (type: SortType) => set({ type }),
  // setDisplayType: (type: DisplayType) => set({ type }),
  setSelectedTab: async (identifier) => {
    set({ selectedTab: identifier });

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
