import { invoke } from "@tauri-apps/api/core";
import { Song } from "@/shared/lib/audio.ts";

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });
