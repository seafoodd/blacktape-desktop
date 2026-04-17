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
  external_cover_url?: string;
  cover?: number[] | null;
};

export type ArtistSummary = {
  name: string;
  album_count: number;
  cover_url?: string;
};

export type Album = {
  title: string;
  cover_url: string;
  songs: Song[];
};

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const getArtists = (query?: string): Promise<ArtistSummary[]> => {
  if (query) console.log("Queries not supported");
  return invoke<ArtistSummary[]>("get_artists");
};

export const getArtistAlbums = (artistName: string): Promise<Album[]> =>
  invoke<Album[]>("get_artist_albums", { artistName });

export const startPlayback = (
  id: number,
  queue: number[],
  history: number[],
): Promise<void> => invoke("start_playback", { id, queue, history });

export const pause = (): Promise<void> => invoke("pause");

export const resume = (): Promise<void> => invoke("resume");

export const toggle = (): Promise<void> => invoke("toggle");

export const seek = (fraction: number): Promise<void> => {
  invoke("seek", { fraction });
  return resume();
};

export const next = (): Promise<void> => invoke("next");

export const previous = (): Promise<void> => invoke("previous");

export const getPosition = (): Promise<number> => {
  return invoke<number>("get_position");
};
export const isPaused = (): Promise<boolean> => {
  return invoke<boolean>("get_is_paused");
};
