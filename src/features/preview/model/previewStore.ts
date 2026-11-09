import { create } from "zustand";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  IDLE_PREVIEW,
  previewPlay,
  previewPrefetch,
  previewSeek,
  previewStop,
  previewToggle,
  type PreviewState,
} from "@/shared/lib/preview.ts";

/** Hovering a track this long (ms) warms its stream url, so the click that follows starts faster. */
export const PREFETCH_HOVER_MS = 120;

export interface PreviewStore {
  state: PreviewState;
  /** Applies a backend `preview-state` event. */
  apply: (state: PreviewState) => void;
  /** Starts the preview of `item`; clicking the item that is already previewed toggles pause. */
  play: (item: SearchSuggestion) => Promise<void>;
  toggle: () => Promise<void>;
  seek: (fraction: number) => Promise<void>;
  stop: () => Promise<void>;
  /** Warms the url cache of `item` (fire and forget). */
  prefetch: (item: SearchSuggestion) => void;
}

/** True when a preview of `url` is active (loading, playing or paused). */
export function isPreviewing(state: PreviewState, url: string): boolean {
  return (
    state.url === url &&
    (state.status === "loading" || state.status === "playing" || state.status === "paused")
  );
}

const prefetched = new Set<string>();

export const usePreviewStore = create<PreviewStore>((set, get) => ({
  state: IDLE_PREVIEW,

  apply: (state) => set({ state }),

  play: async (item) => {
    const current = get().state;
    if (isPreviewing(current, item.item_url_path) && current.status !== "loading") {
      await get().toggle();
      return;
    }
    // Instant feedback; the backend events take over from here.
    set({
      state: {
        ...IDLE_PREVIEW,
        status: "loading",
        platform: item.platform,
        url: item.item_url_path,
        title: item.name,
        artist: item.band_name,
        cover: item.img || null,
        duration_ms: typeof item.duration === "number" ? item.duration * 1000 : null,
      },
    });
    try {
      await previewPlay({
        platform: item.platform,
        url: item.item_url_path,
        title: item.name,
        artist: item.band_name,
        cover: item.img || null,
        duration_ms: typeof item.duration === "number" ? item.duration * 1000 : null,
      });
    } catch (error) {
      // The backend already emitted the error state unless the call never reached it.
      if (get().state.url === item.item_url_path && get().state.status === "loading") {
        set((s) => ({
          state: { ...s.state, status: "error", error: String(error) },
        }));
      }
    }
  },

  toggle: async () => {
    await previewToggle();
  },

  seek: async (fraction) => {
    const duration = get().state.duration_ms;
    if (duration) {
      // Move the bar right away; the backend confirms.
      set((s) => ({
        state: { ...s.state, position_ms: Math.round(duration * fraction) },
      }));
    }
    await previewSeek(fraction).catch(() => undefined);
  },

  stop: async () => {
    set({ state: IDLE_PREVIEW });
    await previewStop();
  },

  prefetch: (item) => {
    const key = `${item.platform}|${item.item_url_path}`;
    if (prefetched.has(key)) return;
    prefetched.add(key);
    void previewPrefetch(item.platform, item.item_url_path);
  },
}));
