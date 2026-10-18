import { invoke } from "@tauri-apps/api/core";
import { Playlist, Song } from "@/shared/types";

export const playlistApi = {
  getAll: () => invoke<Playlist[]>("get_all_playlists"),
  getById: (playlistId: number) =>
    invoke<Playlist | null>("get_playlist_by_id", { playlistId }),
  create: (name: string, description?: string, coverUrl?: string) =>
    invoke<number>("create_playlist", { name, description, coverUrl }),
  update: (
    playlistId: number,
    name?: string,
    description?: string,
    coverUrl?: string,
  ) =>
    invoke<void>("update_playlist_details", {
      playlistId,
      name,
      description,
      coverUrl,
    }),
  delete: (playlistId: number) =>
    invoke<void>("delete_playlist", { playlistId }),
  getSongs: (playlistId: number) =>
    invoke<Song[]>("get_playlist_songs", { playlistId }),
  addSongs: (playlistId: number, songIds: number[]) =>
    invoke<void>("add_songs_to_playlist", { playlistId, songIds }),
  removeSongs: async (playlistId: number, positions: number[]) => {
    await invoke("remove_songs_from_playlist", { playlistId, positions });
  },
  reorderSongs: async (
    playlistId: number,
    fromPosition: number,
    toPosition: number,
  ) => {
    await invoke("reorder_playlist_song", {
      playlistId,
      fromPosition,
      toPosition,
    });
  },
};
