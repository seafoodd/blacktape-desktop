import { invoke } from "@tauri-apps/api/core";

export type Duration = {
  secs: number;
  nanos: number;
};

export type Song = {
  path: string;
  title: string;
  artist: string;
  album: string;
  duration: Duration;
  cover?: number[] | null;
};

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const playSong = (song: Song): Promise<void> =>
  invoke("play_song", { song });

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
