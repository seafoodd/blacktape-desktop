import type { Song } from "@/shared/types.ts";
import type { NavEntry } from "@/features/navigation/model/navigationStore.ts";

export const STORAGE_KEY_VOLUME = "blacktape_player_volume";
export const STORAGE_KEY_PLAYER_STATE = "blacktape_player_state";
export const STORAGE_KEY_NAV_STATE = "blacktape_nav_state";

export interface PersistedPlayerState {
  currentSong: Song | null;
  queue: Song[];
  queueCursor: number | null;
  progress: number;
}

export function parseSavedVolume(raw: string | null): number | null {
  if (raw === null || raw === undefined || raw.trim() === "") return null;
  const num = parseFloat(raw);
  if (!Number.isFinite(num) || isNaN(num)) return null;
  if (num < 0 || num > 1) return null;
  return num;
}

export function parseSavedPlayerState(
  raw: string | null,
): PersistedPlayerState | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;

    const currentSong =
      parsed.currentSong &&
      typeof parsed.currentSong === "object" &&
      typeof (parsed.currentSong as Song).title === "string"
        ? (parsed.currentSong as Song)
        : null;

    const queue = Array.isArray(parsed.queue)
      ? (parsed.queue as unknown[]).filter(
          (s): s is Song =>
            !!s &&
            typeof s === "object" &&
            typeof (s as Song).title === "string",
        )
      : [];

    const queueCursor =
      typeof parsed.queueCursor === "number" &&
      Number.isInteger(parsed.queueCursor) &&
      parsed.queueCursor >= 0
        ? parsed.queueCursor
        : null;

    const progress =
      typeof parsed.progress === "number" &&
      Number.isFinite(parsed.progress) &&
      parsed.progress >= 0 &&
      parsed.progress <= 1
        ? parsed.progress
        : 0;

    if (!currentSong && queue.length === 0) {
      return null;
    }

    return {
      currentSong,
      queue,
      queueCursor,
      progress,
    };
  } catch {
    return null;
  }
}

export function parseSavedNavEntry(raw: string | null): NavEntry | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return null;
    if (typeof parsed.view !== "string") return null;

    const validViews = [
      "ARTIST_ALBUMS",
      "SEARCH_RESULTS",
      "ALBUM_PREVIEW",
      "PLAYLIST",
      "TRASH",
    ];
    if (!validViews.includes(parsed.view)) return null;

    return {
      view: parsed.view,
      artistName:
        typeof parsed.artistName === "string" ? parsed.artistName : undefined,
      searchQuery:
        typeof parsed.searchQuery === "string"
          ? parsed.searchQuery
          : undefined,
      playlistId:
        typeof parsed.playlistId === "number" ? parsed.playlistId : undefined,
      previewItem:
        parsed.previewItem && typeof parsed.previewItem === "object"
          ? parsed.previewItem
          : undefined,
    };
  } catch {
    return null;
  }
}

function getStorage(): Storage | null {
  try {
    if (
      typeof window !== "undefined" &&
      typeof window.localStorage !== "undefined" &&
      typeof window.localStorage.getItem === "function"
    ) {
      return window.localStorage;
    }
    if (
      typeof localStorage !== "undefined" &&
      typeof localStorage.getItem === "function"
    ) {
      return localStorage;
    }
  } catch {}
  return null;
}

export function loadSavedVolume(): number | null {
  const storage = getStorage();
  if (!storage) return null;
  try {
    return parseSavedVolume(storage.getItem(STORAGE_KEY_VOLUME));
  } catch {
    return null;
  }
}

export function saveVolume(volume: number): void {
  const storage = getStorage();
  if (!storage) return;
  try {
    if (Number.isFinite(volume) && volume >= 0 && volume <= 1) {
      storage.setItem(STORAGE_KEY_VOLUME, String(volume));
    }
  } catch (err) {
    console.warn("Failed to persist volume:", err);
  }
}

export function loadSavedPlayerState(): PersistedPlayerState | null {
  const storage = getStorage();
  if (!storage) return null;
  try {
    return parseSavedPlayerState(storage.getItem(STORAGE_KEY_PLAYER_STATE));
  } catch {
    return null;
  }
}

export function savePlayerState(state: PersistedPlayerState): void {
  const storage = getStorage();
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY_PLAYER_STATE, JSON.stringify(state));
  } catch (err) {
    console.warn("Failed to persist player state:", err);
  }
}

export function loadSavedNavEntry(): NavEntry | null {
  const storage = getStorage();
  if (!storage) return null;
  try {
    return parseSavedNavEntry(storage.getItem(STORAGE_KEY_NAV_STATE));
  } catch {
    return null;
  }
}

export function saveNavEntry(entry: NavEntry): void {
  const storage = getStorage();
  if (!storage) return;
  try {
    storage.setItem(STORAGE_KEY_NAV_STATE, JSON.stringify(entry));
  } catch (err) {
    console.warn("Failed to persist nav state:", err);
  }
}
