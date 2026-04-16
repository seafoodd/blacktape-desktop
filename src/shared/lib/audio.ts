import { invoke } from "@tauri-apps/api/core";

export type Song = {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  duration_ms: number;
  track_number?: number;
  genre?: string;
  release_year?: string;
  cover_url?: string;
  cover?: number[] | null;
};

export type ArtistSummary = {
  name: string;
  album_count: number;
  cover_url?: string;
}

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const getArtists = (): Promise<ArtistSummary[]> =>
  invoke<ArtistSummary[]>("get_artists");

export const playSong = (id: number): Promise<void> =>
  invoke("play_song", { id });

export const pause = (): Promise<void> => invoke("pause");

export const resume = (): Promise<void> => invoke("resume");

export const toggle = (): Promise<void> => invoke("toggle");

export const seek = (fraction: number): void => {
  invoke("seek", { fraction });
  resume();
};
export const getPosition = (): Promise<number> => {
  return invoke<number>("get_position");
};
export const isPaused = (): Promise<boolean> => {
  return invoke<boolean>("get_is_paused");
};
