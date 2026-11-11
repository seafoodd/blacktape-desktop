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
  /** Album title (shown by the player) and the page url of the album (not sent: kept by `streamAlbumUrl`). */
  album?: string | null;
  album_url?: string | null;
  cover?: string | null;
  duration_ms?: number | null;
}

/** Page url of the album of every stream started so far, by track url. The player only knows the track. */
const albumUrls = new Map<string, string>();

/** Remembers the album pages of `requests` so the player can link its current stream to them. */
export function rememberStreamAlbums(requests: readonly Pick<StreamRequest, "url" | "album_url">[]): void {
  for (const r of requests) if (r.album_url) albumUrls.set(r.url, r.album_url);
}

/** Page url of the album of the streamed track `url`, undefined when unknown. */
export const streamAlbumUrl = (url: string | null | undefined): string | undefined =>
  url ? albumUrls.get(url) : undefined;

/**
 * Replaces the queue with stream songs and starts `index`. Returns at once: the player opens the song in the
 * background (`stream_loading` in the player state) and emits `stream-error` when it cannot be played.
 * `shuffle: true` starts at a random song with shuffle mode on (`index` is ignored).
 */
export const streamQueue = (requests: StreamRequest[], index: number, shuffle = false): Promise<void> => {
  rememberStreamAlbums(requests);
  return invoke("stream_queue", { requests, index, shuffle });
};

/**
 * Adds stream songs to the queue without replacing it: right after the current song when `playNext`, at the
 * end otherwise. Starts playing them when nothing is queued.
 */
export const streamAddToQueue = (requests: StreamRequest[], playNext = false): Promise<void> => {
  rememberStreamAlbums(requests);
  return invoke("stream_add_to_queue", { requests, playNext });
};

/** An entry of a mixed queue: a library song or a song to stream. */
export type MixedEntry = { kind: "song"; id: number } | ({ kind: "stream" } & StreamRequest);

/**
 * Replaces the queue with library songs and streams in the given order and starts `index`. The entries before
 * it stay in the queue (reachable with "previous").
 */
export const startMixedPlayback = (entries: MixedEntry[], index: number): Promise<void> => {
  rememberStreamAlbums(entries.filter((e): e is { kind: "stream" } & StreamRequest => e.kind === "stream"));
  return invoke("start_mixed_playback", { entries, index });
};

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
