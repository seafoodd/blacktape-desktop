import { create } from "zustand";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { streamPlay, streamPrefetch } from "@/shared/lib/stream.ts";

/** Hovering a track this long (ms) warms its stream url, so the click that follows starts faster. */
export const PREFETCH_HOVER_MS = 60;

/** How long an error stays on the button (ms). */
export const ERROR_VISIBLE_MS = 5000;

export interface StreamStore {
  /** Url of the item that is being resolved/opened right now. */
  loadingUrl: string | null;
  /** Last failure, shown as the tooltip of the item's button. */
  error: { url: string; message: string } | null;
  /** Streams `item`; pressing the item that is already streaming toggles pause. */
  play: (item: SearchSuggestion) => Promise<void>;
  /** Warms the url cache of `item` (fire and forget). */
  prefetch: (item: SearchSuggestion) => void;
}

/** True when the player currently streams the track of `url`. */
export function isStreamingUrl(
  current: { source_url?: string | null } | null,
  isStreaming: boolean,
  url: string,
): boolean {
  return isStreaming && current?.source_url === url;
}

const prefetched = new Set<string>();
let errorTimer: ReturnType<typeof setTimeout> | undefined;

export const useStreamStore = create<StreamStore>((set, get) => ({
  loadingUrl: null,
  error: null,

  play: async (item) => {
    const audio = useAudioStore.getState();
    const url = item.item_url_path;
    if (isStreamingUrl(audio.currentSong, audio.isStreaming, url)) {
      await audio.togglePlay();
      return;
    }
    clearTimeout(errorTimer);
    set({ loadingUrl: url, error: null });
    try {
      await streamPlay({
        platform: item.platform,
        url,
        title: item.name,
        artist: item.band_name,
        cover: item.img || null,
        duration_ms: typeof item.duration === "number" ? item.duration * 1000 : null,
      });
    } catch (error) {
      const message = typeof error === "string" ? error : String(error);
      set({ error: { url, message } });
      errorTimer = setTimeout(() => {
        if (get().error?.url === url) set({ error: null });
      }, ERROR_VISIBLE_MS);
    } finally {
      // A newer click may have taken over the loading slot.
      if (get().loadingUrl === url) set({ loadingUrl: null });
    }
  },

  prefetch: (item) => {
    const key = `${item.platform}|${item.item_url_path}`;
    if (prefetched.has(key)) return;
    prefetched.add(key);
    void streamPrefetch(item.platform, item.item_url_path);
  },
}));
