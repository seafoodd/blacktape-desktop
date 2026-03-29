import { invoke } from "@tauri-apps/api/core";

export type Song = {
  path: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
  cover?: number[] | null;
};

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const playSong = (song: Song): Promise<void> =>
  invoke("play_song", { song });

export const pause = (): Promise<void> => invoke("pause");

export const resume = (): Promise<void> => invoke("resume");

export const seek = (fraction: number): Promise<void> =>
  invoke("seek", { fraction });

export const getPosition = (): Promise<number> =>
  invoke<number>("get_position");
