import { invoke } from "@tauri-apps/api/core";
import { Song } from "@/shared/types.ts";

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });
