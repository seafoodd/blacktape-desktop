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
 * `shuffle: true` starts at a random song with shuffle mode on (`index` is ignored).
 */
export const streamQueue = (requests: StreamRequest[], index: number, shuffle = false): Promise<void> =>
  invoke("stream_queue", { requests, index, shuffle });

/**
 * Adds stream songs to the queue without replacing it: right after the current song when `playNext`, at the
 * end otherwise. Starts playing them when nothing is queued.
 */
export const streamAddToQueue = (requests: StreamRequest[], playNext = false): Promise<void> =>
  invoke("stream_add_to_queue", { requests, playNext });

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
