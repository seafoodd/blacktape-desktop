import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import {
  streamPrefetch,
  streamQueue,
  type StreamError,
  type StreamRequest,
} from "@/shared/lib/stream.ts";

/** Hovering a track this long (ms) warms its stream url, so the click that follows starts faster. */
export const PREFETCH_HOVER_MS = 60;

/** How long an error stays on the button (ms). */
export const ERROR_VISIBLE_MS = 5000;

export interface StreamStore {
  /** Url of the item clicked last, until the player reports it (instant spinner). */
  pendingUrl: string | null;
  /** Last failure, shown as the tooltip of the item's button. */
  error: StreamError | null;
  /**
   * Streams `item`: the queue becomes `item` followed by the items after it in `list` (the visible results).
   * Pressing the item that is already playing toggles pause.
   */
  play: (item: SearchSuggestion, list?: readonly SearchSuggestion[]) => Promise<void>;
  /** Warms the url cache of `item` (fire and forget). */
  prefetch: (item: SearchSuggestion) => void;
  /** Applies a `stream-error` event. */
  fail: (error: StreamError) => void;
}

/** True when the player's current song is the stream of `url`. */
export function isStreamingUrl(
  current: { source_url?: string | null } | null,
  isStreaming: boolean,
  url: string,
): boolean {
  return isStreaming && current?.source_url === url;
}

export function toStreamRequest(item: SearchSuggestion): StreamRequest {
  return {
    platform: item.platform,
    url: item.item_url_path,
    title: item.name,
    artist: item.band_name,
    cover: item.img || null,
    duration_ms: typeof item.duration === "number" ? item.duration * 1000 : null,
  };
}

/** `item` followed by the tracks after it in `list` (deduplicated by url); just `item` when it is not in `list`. */
export function queueFrom(
  item: SearchSuggestion,
  list: readonly SearchSuggestion[] = [],
): SearchSuggestion[] {
  const start = list.findIndex((t) => t.item_url_path === item.item_url_path);
  const rest = start === -1 ? [] : list.slice(start + 1);
  const seen = new Set<string>([item.item_url_path]);
  const out = [item];
  for (const track of rest) {
    if (seen.has(track.item_url_path)) continue;
    seen.add(track.item_url_path);
    out.push(track);
  }
  return out;
}

const prefetched = new Set<string>();
let errorTimer: ReturnType<typeof setTimeout> | undefined;

export const useStreamStore = create<StreamStore>((set, get) => ({
  pendingUrl: null,
  error: null,

  play: async (item, list) => {
    const audio = useAudioStore.getState();
    const url = item.item_url_path;
    if (isStreamingUrl(audio.currentSong, audio.isStreaming, url)) {
      await audio.togglePlay();
      return;
    }
    clearTimeout(errorTimer);
    set({ pendingUrl: url, error: null });
    try {
      await streamQueue(queueFrom(item, list).map(toStreamRequest), 0);
    } catch (error) {
      get().fail({ url, message: typeof error === "string" ? error : String(error) });
    } finally {
      if (get().pendingUrl === url) set({ pendingUrl: null });
    }
  },

  prefetch: (item) => {
    const key = `${item.platform}|${item.item_url_path}`;
    if (prefetched.has(key)) return;
    prefetched.add(key);
    void streamPrefetch(item.platform, item.item_url_path);
  },

  fail: (error) => {
    clearTimeout(errorTimer);
    set({ error });
    errorTimer = setTimeout(() => {
      if (get().error?.url === error.url) set({ error: null });
    }, ERROR_VISIBLE_MS);
  },
}));

if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
  listen<StreamError>("stream-error", (event) => useStreamStore.getState().fail(event.payload)).catch(
    (e) => console.log(e),
  );
}
