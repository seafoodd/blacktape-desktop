import { invoke } from "@tauri-apps/api/core";
import { Album, ArtistSummary, RepeatMode, Song } from "@/shared/types";

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const getArtists = (query?: string): Promise<ArtistSummary[]> => {
  if (query) console.log("Queries not supported");
  return invoke<ArtistSummary[]>("get_artists");
};

export const getArtistAlbums = (artistName: string): Promise<Album[]> =>
  invoke<Album[]>("get_artist_albums", { artistName });

export const startPlayback = (
  queue: number[],
  currentIndex: number,
): Promise<void> => invoke("start_playback", { queue, currentIndex });

export const playSongInQueue = (index: number): Promise<void> =>
  invoke("play_song_in_queue", { index });

export const pause = (): Promise<void> => invoke("pause");

export const resume = (): Promise<void> => invoke("resume");

export const toggle = (): Promise<void> => invoke("toggle");

export const seek = (fraction: number): Promise<void> => {
  invoke("seek", { fraction });
  return resume();
};

export const next = (): Promise<void> => invoke("next");

export const previous = (): Promise<void> => invoke("previous");

export const AddToQueue = (id: number): Promise<void> =>
  invoke("add_to_queue", { songId: id });

export const AddNextToQueue = (id: number): Promise<void> =>
  invoke("add_next_to_queue", { songId: id });

export const getPosition = (): Promise<number> => {
  return invoke<number>("get_position");
};

export const isPaused = (): Promise<boolean> => {
  return invoke<boolean>("get_is_paused");
};

export const setVolume = (fraction: number): Promise<void> =>
  invoke("set_volume", { fraction });

export const getVolume = (): Promise<void> => invoke("get_volume");

export const fetchState = (): void => {
  invoke("fetch_state");
};
export const toggleShuffle = (): void => {
  invoke("toggle_shuffle");
};

export const setRepeatMode = (repeatMode: RepeatMode): void => {
  invoke("set_repeat_mode", { repeatMode: repeatMode });
};
