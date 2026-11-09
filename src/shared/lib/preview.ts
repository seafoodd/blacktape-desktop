/**
 * Invoke wrappers + types of the streaming preview. The types mirror `PreviewState` / `PreviewRequest`
 * in `src-tauri/src/preview/commands.rs` (serde, snake_case).
 */
import { invoke } from "@tauri-apps/api/core";
import type { Platform } from "@/shared/types.ts";

export type PreviewStatus = "idle" | "loading" | "playing" | "paused" | "error";

export interface PreviewState {
  status: PreviewStatus;
  platform: Platform | null;
  url: string | null;
  title: string;
  artist: string;
  cover: string | null;
  position_ms: number;
  duration_ms: number | null;
  error: string | null;
  /** Click to first sound, once playing. */
  startup_ms: number | null;
}

export interface PreviewRequest {
  platform: Platform;
  url: string;
  title: string;
  artist: string;
  cover?: string | null;
  duration_ms?: number | null;
}

export const IDLE_PREVIEW: PreviewState = {
  status: "idle",
  platform: null,
  url: null,
  title: "",
  artist: "",
  cover: null,
  position_ms: 0,
  duration_ms: null,
  error: null,
  startup_ms: null,
};

export const previewPlay = (request: PreviewRequest): Promise<void> =>
  invoke("preview_play", { request });

/** Warms the stream url cache; never rejects. */
export const previewPrefetch = async (platform: Platform, url: string): Promise<void> => {
  try {
    await invoke<void>("preview_prefetch", { platform, url });
  } catch {
    // best effort: a failed warm-up only means the click resolves the url itself
  }
};

export const previewToggle = (): Promise<void> => invoke("preview_toggle");
export const previewSeek = (fraction: number): Promise<void> =>
  invoke("preview_seek", { fraction });
export const previewStop = (): Promise<void> => invoke("preview_stop");
export const previewGetState = (): Promise<PreviewState> => invoke("preview_get_state");
