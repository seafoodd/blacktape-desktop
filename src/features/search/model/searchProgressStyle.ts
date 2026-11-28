/**
 * Where the search bar shows the progress of a search. Stored per machine in localStorage; a change made in the
 * settings window reaches the main window through the `storage` event.
 */
import { useSyncExternalStore } from "react";

export const SEARCH_PROGRESS_STYLE_KEY = "search_progress_style";

export const SEARCH_PROGRESS_STYLES = [
  {
    id: "pulse",
    label: "Pulse line",
    description:
      "A thin line under the search field and a counter. Point at the counter to see each source.",
  },
  {
    id: "dashboard",
    label: "Source dashboard",
    description: "Cards above the results with every source, its time and what it found.",
  },
] as const;

export type SearchProgressStyle = (typeof SEARCH_PROGRESS_STYLES)[number]["id"];

const DEFAULT_STYLE: SearchProgressStyle = "pulse";

/** The stored value as one of the styles; anything else (missing, garbage, removed style) is the default. */
export function parseSearchProgressStyle(raw: string | null | undefined): SearchProgressStyle {
  return SEARCH_PROGRESS_STYLES.find((style) => style.id === raw)?.id ?? DEFAULT_STYLE;
}

export function readSearchProgressStyle(): SearchProgressStyle {
  try {
    return parseSearchProgressStyle(localStorage.getItem(SEARCH_PROGRESS_STYLE_KEY));
  } catch {
    return DEFAULT_STYLE;
  }
}

/** Listeners of this window: the `storage` event only reaches the other ones. */
const listeners = new Set<() => void>();

export function writeSearchProgressStyle(style: SearchProgressStyle): void {
  try {
    localStorage.setItem(SEARCH_PROGRESS_STYLE_KEY, style);
  } catch (err) {
    console.error("Failed to save the search progress style:", err);
  }
  listeners.forEach((listener) => listener());
}

export function subscribeSearchProgressStyle(onChange: () => void): () => void {
  const onStorage = (event: StorageEvent) => {
    // A null key means the whole storage was cleared.
    if (event.key === SEARCH_PROGRESS_STYLE_KEY || event.key === null) onChange();
  };
  listeners.add(onChange);
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(onChange);
    window.removeEventListener("storage", onStorage);
  };
}

export const useSearchProgressStyle = (): SearchProgressStyle =>
  useSyncExternalStore(subscribeSearchProgressStyle, readSearchProgressStyle);
