/**
 * Invoke wrappers of the streaming playback. `StreamRequest` mirrors `StreamRequest` in
 * `src-tauri/src/preview/commands.rs` (serde, snake_case). The tracks then play through the normal player
 * and queue, so their state arrives with the usual `player-state` event (`streaming`, `stream_loading`).
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

/**
 * Replaces the queue with stream songs and starts `index`. Returns at once: the player opens the song in the
 * background (`stream_loading` in the player state) and emits `stream-error` when it cannot be played.
 */
export const streamQueue = (requests: StreamRequest[], index: number): Promise<void> =>
  invoke("stream_queue", { requests, index });

export interface StreamError {
  url: string;
  message: string;
}

/** Warms the stream url cache; never rejects. */
export const streamPrefetch = async (platform: Platform, url: string): Promise<void> => {
  try {
    await invoke<void>("stream_prefetch", { platform, url });
  } catch {
    // best effort: a failed warm-up only means the click resolves the url itself
  }
};
