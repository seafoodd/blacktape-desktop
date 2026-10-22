// src/shared/store/playlistStore.ts
import { create } from "zustand";
import { playlistApi } from "@/shared/api/playlists";
import { Playlist, Song } from "@/shared/types";

interface PlaylistState {
  playlists: Playlist[];
  activePlaylist: Playlist | null;
  activePlaylistId: number | null;
  activePlaylistSongs: Song[];
  loading: boolean;

  fetchPlaylists: () => Promise<void>;
  selectPlaylist: (playlistId: number) => Promise<void>;
  unselectPlaylist: () => void;
  createPlaylist: (
    name: string,
    description?: string,
    coverUrl?: string,
  ) => Promise<number>;
  updatePlaylist: (
    playlistId: number,
    name?: string,
    description?: string,
    coverUrl?: string,
  ) => Promise<void>;
  deletePlaylist: (playlistId: number) => Promise<void>;
  addSongs: (playlistId: number, songIds: number[]) => Promise<void>;
  removeSongs: (playlistId: number, positions: number[]) => Promise<void>;
  reorderSongs: (
    playlistId: number,
    fromPosition: number,
    toPosition: number,
  ) => Promise<void>;
}

export const usePlaylistStore = create<PlaylistState>((set, get) => ({
  playlists: [],
  activePlaylist: null,
  activePlaylistId: null,
  activePlaylistSongs: [],
  loading: false,

  fetchPlaylists: async () => {
    try {
      set({ loading: true });
      const playlists = await playlistApi.getAll();
      set({ playlists, loading: false });
    } catch (error) {
      console.error("Failed to fetch playlists:", error);
      set({ loading: false });
    }
  },

  selectPlaylist: async (playlistId) => {
    try {
      set({ loading: true });
      const [playlist, songs] = await Promise.all([
        playlistApi.getById(playlistId),
        playlistApi.getSongs(playlistId),
      ]);
      set({
        activePlaylist: playlist,
        activePlaylistId: playlistId,
        activePlaylistSongs: songs,
        loading: false,
      });
    } catch (error) {
      console.error("Failed to load playlist details:", error);
      set({ loading: false });
    }
  },
  unselectPlaylist: () => {
    set({
      activePlaylist: null,
      activePlaylistId: null,
      activePlaylistSongs: [],
    });
  },

  createPlaylist: async (name, description, coverUrl): Promise<number> => {
    const createdPlaylistId = await playlistApi.create(
      name,
      description,
      coverUrl,
    );
    await get().fetchPlaylists();
    return createdPlaylistId;
  },

  updatePlaylist: async (playlistId, name, description, coverUrl) => {
    await playlistApi.update(playlistId, name, description, coverUrl);
    await Promise.all([
      get().fetchPlaylists(),
      get().selectPlaylist(playlistId),
    ]);
  },

  deletePlaylist: async (playlistId) => {
    await playlistApi.delete(playlistId);
    if (get().activePlaylist?.id === playlistId) {
      set({ activePlaylist: null, activePlaylistSongs: [] });
    }
    await get().fetchPlaylists();
  },

  addSongs: async (playlistId, songIds) => {
    await playlistApi.addSongs(playlistId, songIds);
    await get().selectPlaylist(playlistId);
  },

  removeSongs: async (playlistId: number, positions: number[]) => {
    const currentSongs = get().activePlaylistSongs;
    const positionsSet = new Set(positions);

    // Optimistic update
    const newSongs = currentSongs.filter(
      (_, index) => !positionsSet.has(index),
    );
    set({ activePlaylistSongs: newSongs });

    try {
      await playlistApi.removeSongs(playlistId, positions);
    } catch (error) {
      console.error("Failed to remove songs:", error);
      set({ activePlaylistSongs: currentSongs });
    }
  },

  reorderSongs: async (
    playlistId: number,
    fromPosition: number,
    toPosition: number,
  ) => {
    const currentSongs = get().activePlaylistSongs;

    if (
      !Number.isInteger(fromPosition) ||
      !Number.isInteger(toPosition) ||
      fromPosition < 0 ||
      toPosition < 0 ||
      fromPosition >= currentSongs.length ||
      toPosition >= currentSongs.length
    ) {
      throw new Error("Invalid playlist reorder positions");
    }

    if (fromPosition === toPosition) return;

    const newSongs = [...currentSongs];
    const [moved] = newSongs.splice(fromPosition, 1);

    if (moved === undefined) {
      throw new Error("Song not found at source position");
    }

    newSongs.splice(toPosition, 0, moved);

    set({ activePlaylistSongs: newSongs });

    try {
      await playlistApi.reorderSongs(
        playlistId,
        fromPosition + 1,
        toPosition + 1,
      );
    } catch (error) {
      if (get().activePlaylistSongs === newSongs) {
        set({ activePlaylistSongs: currentSongs });
      }

      throw error;
    }
  },
}));
