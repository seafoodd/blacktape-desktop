/**
 * Invoke wrappers of the streaming playback. `StreamRequest` mirrors `StreamRequest` in
 * `src-tauri/src/preview/commands.rs` (serde, snake_case). The track then plays through the normal player,
 * so its state arrives with the usual `player-state` event (`streaming: true`).
 */
import { invoke } from "@tauri-apps/api/core";
import type { Platform } from "@/shared/types.ts";

export interface StreamRequest {
  platform: Platform;
  url: string;
  title: string;
  artist: string;
  cover?: string | null;
  duration_ms?: number | null;
}

/** Resolves, opens and plays the track. Rejects with a message when it cannot be streamed. */
export const streamPlay = (request: StreamRequest): Promise<void> =>
  invoke("stream_play", { request });

/** Warms the stream url cache; never rejects. */
export const streamPrefetch = async (platform: Platform, url: string): Promise<void> => {
  try {
    await invoke<void>("stream_prefetch", { platform, url });
  } catch {
    // best effort: a failed warm-up only means the click resolves the url itself
  }
};
