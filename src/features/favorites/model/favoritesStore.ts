import { create } from "zustand";
import { Song } from "@/shared/types.ts";
import {
  getFavoriteIds,
  getFavoriteSongs,
  toggleFavorite as toggleFavoriteApi,
  addFavorites as addFavoritesApi,
  removeFavorites as removeFavoritesApi,
} from "@/shared/lib/audio.ts";

interface FavoritesState {
  favoriteIds: Set<number>;
  favoriteSongs: Song[];
  loading: boolean;

  fetchFavoriteIds: () => Promise<void>;
  fetchFavoriteSongs: () => Promise<void>;
  isFavorite: (songId: number) => boolean;
  toggleFavorite: (songId: number) => Promise<boolean>;
  addFavorites: (songIds: number[]) => Promise<void>;
  removeFavorites: (songIds: number[]) => Promise<void>;
}

export const useFavoritesStore = create<FavoritesState>((set, get) => ({
  favoriteIds: new Set<number>(),
  favoriteSongs: [],
  loading: false,

  fetchFavoriteIds: async () => {
    try {
      const ids = await getFavoriteIds();
      set({ favoriteIds: new Set(ids) });
    } catch (err) {
      console.error("Failed to fetch favorite ids:", err);
    }
  },

  fetchFavoriteSongs: async () => {
    try {
      set({ loading: true });
      const songs = await getFavoriteSongs();
      set({
        favoriteSongs: songs,
        favoriteIds: new Set(songs.map((s) => s.id)),
        loading: false,
      });
    } catch (err) {
      console.error("Failed to fetch favorite songs:", err);
      set({ loading: false });
    }
  },

  isFavorite: (songId: number) => {
    return get().favoriteIds.has(songId);
  },

  toggleFavorite: async (songId: number): Promise<boolean> => {
    const prevIds = get().favoriteIds;
    const prevSongs = get().favoriteSongs;
    const wasFavorite = prevIds.has(songId);

    // Optimistic update
    const nextIds = new Set(prevIds);
    let nextSongs = prevSongs;
    if (wasFavorite) {
      nextIds.delete(songId);
      nextSongs = prevSongs.filter((s) => s.id !== songId);
    } else {
      nextIds.add(songId);
    }
    set({ favoriteIds: nextIds, favoriteSongs: nextSongs });

    try {
      const isNowFavorite = await toggleFavoriteApi(songId);
      // If the song was added and wasn't previously in favoriteSongs, or if state differs, re-fetch songs
      if (isNowFavorite && !wasFavorite) {
        // Fetch to ensure full song data and correct ordering
        void get().fetchFavoriteSongs();
      }
      return isNowFavorite;
    } catch (err) {
      console.error("Failed to toggle favorite:", err);
      // Revert optimistic update
      set({ favoriteIds: prevIds, favoriteSongs: prevSongs });
      throw err;
    }
  },

  addFavorites: async (songIds: number[]) => {
    if (songIds.length === 0) return;
    try {
      await addFavoritesApi(songIds);
      await Promise.all([get().fetchFavoriteIds(), get().fetchFavoriteSongs()]);
    } catch (err) {
      console.error("Failed to add favorites:", err);
      throw err;
    }
  },

  removeFavorites: async (songIds: number[]) => {
    if (songIds.length === 0) return;
    try {
      await removeFavoritesApi(songIds);
      await Promise.all([get().fetchFavoriteIds(), get().fetchFavoriteSongs()]);
    } catch (err) {
      console.error("Failed to remove favorites:", err);
      throw err;
    }
  },
}));
