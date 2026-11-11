import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import {
  type AlbumStatus,
  type DownloadState,
  getAlbumStatus,
  getDownloadStatus,
  type RemoteRef,
  refKey,
} from "@/shared/lib/library/libraryIndex.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";

/**
 * Frontend cache of the library index (see DECISIONS D4/D6). Keys are the remote urls exactly as shown in
 * the UI. Requests are batched: one backend call per ~50 ms window (max 200 refs per call), answers are
 * cached until `invalidate()`, which runs on `download-task-completed` and on every library refresh.
 */
export interface LibraryIndexStore {
  /** Track url -> state. A missing key means "not asked yet" (treated as "none" by the hooks). */
  tracks: Record<string, DownloadState>;
  /** Album url -> status. */
  albums: Record<string, AlbumStatus>;
  /** Asks about track urls that are not cached yet (batched, debounced). Safe to call on every render. */
  requestTracks: (refs: readonly RemoteRef[]) => void;
  /** Asks about album urls that are not cached yet (batched, debounced). */
  requestAlbums: (refs: readonly RemoteRef[]) => void;
  /**
   * The library changed: re-ask for every url that was requested. Old answers stay until the new ones arrive
   * (no blink); answers of urls nobody asks about any more are dropped.
   */
  invalidate: () => void;
}

export const REQUEST_WINDOW_MS = 50;
export const CHUNK_SIZE = 200;
/** How many of the most recently asked urls are re-asked on invalidate. */
export const WATCH_LIMIT = 500;
/** Library mutations come in bursts (a download ends with a refresh); invalidate once the burst is quiet. */
export const LIBRARY_DEBOUNCE_MS = 300;

const hasOwn = (record: object, key: string): boolean =>
  Object.prototype.hasOwnProperty.call(record, key);

/** Splits `items` into consecutive pieces of at most `size` items (a size below 1 is treated as 1). */
export function chunk<T>(items: readonly T[], size: number): T[][] {
  const step = Math.max(1, Math.floor(size));
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += step) {
    out.push(items.slice(i, i + step));
  }
  return out;
}

/** The refs whose key `skip` does not reject, one per key (first occurrence wins), in the given order. */
export function unseenRefs(
  refs: readonly RemoteRef[],
  skip: (key: string) => boolean,
): RemoteRef[] {
  const seen = new Set<string>();
  const out: RemoteRef[] = [];
  for (const ref of refs) {
    const key = refKey(ref);
    if (seen.has(key) || skip(key)) continue;
    seen.add(key);
    out.push(ref);
  }
  return out;
}

/**
 * Marks the refs as the most recently asked. A Map keeps insertion order, so re-inserting moves a key to the
 * end; beyond `limit` the oldest keys are dropped.
 */
export function touchWatched(
  watched: Map<string, RemoteRef>,
  refs: readonly RemoteRef[],
  limit: number,
): void {
  for (const ref of refs) {
    const key = refKey(ref);
    watched.delete(key);
    watched.set(key, ref);
  }
  while (watched.size > limit) {
    const oldest = watched.keys().next().value;
    if (oldest === undefined) break;
    watched.delete(oldest);
  }
}

/**
 * One value per requested ref. The backend answers every url it was sent, but a key it left out is stored as
 * `fallback` so the same url is not asked again and again.
 */
export function answersFor<V>(
  refs: readonly RemoteRef[],
  answer: Record<string, V> | null | undefined,
  fallback: V,
): Record<string, V> {
  const out: Record<string, V> = {};
  for (const ref of refs) {
    const key = refKey(ref);
    const value = answer && hasOwn(answer, key) ? answer[key] : fallback;
    out[key] = value;
  }
  return out;
}

export interface Batcher {
  /** Queues the refs that are neither cached, queued nor being fetched. Starts the window if needed. */
  request: (refs: readonly RemoteRef[]) => void;
  /** Drops queued and in-flight work (answers still arriving are ignored) and re-asks every watched ref. */
  restart: () => void;
  /** The watched set: urls asked recently. Cached answers of other urls are dropped on invalidate. */
  watchedKeys: () => Set<string>;
}

/**
 * The entries of `record` whose key is in `keep`. Used on invalidate: answers of urls nobody asks about any
 * more are dropped, the others stay visible until the refetch overwrites them.
 */
export function keepKeys<V>(record: Record<string, V>, keep: ReadonlySet<string>): Record<string, V> {
  const out: Record<string, V> = {};
  for (const key of Object.keys(record)) {
    if (keep.has(key)) out[key] = record[key];
  }
  return out;
}

export interface BatcherOptions<V> {
  fetch: (refs: RemoteRef[]) => Promise<Record<string, V>>;
  fallback: V;
  apply: (values: Record<string, V>) => void;
  isCached: (key: string) => boolean;
}

/**
 * Collects requests for one window of REQUEST_WINDOW_MS, sends them in chunks of CHUNK_SIZE and applies the
 * answers. `restart()` bumps an epoch so that answers of an older epoch are dropped.
 */
export function createBatcher<V>(options: BatcherOptions<V>): Batcher {
  const watched = new Map<string, RemoteRef>();
  const queued = new Map<string, RemoteRef>();
  const inFlight = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let epoch = 0;

  const flush = (): void => {
    timer = null;
    const refs = [...queued.values()];
    queued.clear();
    for (const part of chunk(refs, CHUNK_SIZE)) {
      const current = epoch;
      for (const ref of part) inFlight.add(refKey(ref));
      options.fetch(part).then(
        (answer) => {
          if (current !== epoch) return;
          for (const ref of part) inFlight.delete(refKey(ref));
          options.apply(answersFor(part, answer, options.fallback));
        },
        (error: unknown) => {
          if (current !== epoch) return;
          for (const ref of part) inFlight.delete(refKey(ref));
          // Not cached: the next request for these urls asks again.
          console.warn("Library index request failed", error);
        },
      );
    }
  };

  const schedule = (): void => {
    if (timer === null) timer = setTimeout(flush, REQUEST_WINDOW_MS);
  };

  return {
    request(refs) {
      if (refs.length === 0) return;
      touchWatched(watched, refs, WATCH_LIMIT);
      let added = false;
      const fresh = unseenRefs(
        refs,
        (key) => options.isCached(key) || inFlight.has(key) || queued.has(key),
      );
      for (const ref of fresh) {
        queued.set(refKey(ref), ref);
        added = true;
      }
      if (added) schedule();
    },
    restart() {
      epoch += 1;
      inFlight.clear();
      queued.clear();
      for (const ref of watched.values()) queued.set(refKey(ref), ref);
      if (queued.size > 0) schedule();
    },
    watchedKeys: () => new Set(watched.keys()),
  };
}

const trackBatcher = createBatcher<DownloadState>({
  fetch: getDownloadStatus,
  fallback: "none",
  apply: (values) =>
    useLibraryIndexStore.setState((s) => ({ tracks: { ...s.tracks, ...values } })),
  isCached: (key) => hasOwn(useLibraryIndexStore.getState().tracks, key),
});

const NO_ALBUM: AlbumStatus = { state: "none", owned: 0, total: 0 };

const albumBatcher = createBatcher<AlbumStatus>({
  fetch: getAlbumStatus,
  fallback: NO_ALBUM,
  apply: (values) =>
    useLibraryIndexStore.setState((s) => ({ albums: { ...s.albums, ...values } })),
  isCached: (key) => hasOwn(useLibraryIndexStore.getState().albums, key),
});

export const useLibraryIndexStore = create<LibraryIndexStore>(() => ({
  tracks: {},
  albums: {},
  requestTracks: (refs) => trackBatcher.request(refs),
  requestAlbums: (refs) => albumBatcher.request(refs),
  invalidate: () => {
    // Stale-while-revalidate (DECISIONS D24): values stay visible until the re-ask overwrites them, so badges
    // do not blink on every library change. Only urls nobody watches any more are dropped.
    const trackKeep = trackBatcher.watchedKeys();
    const albumKeep = albumBatcher.watchedKeys();
    useLibraryIndexStore.setState((s) => ({
      tracks: keepKeys(s.tracks, trackKeep),
      albums: keepKeys(s.albums, albumKeep),
    }));
    trackBatcher.restart();
    albumBatcher.restart();
  },
}));

let invalidateTimer: ReturnType<typeof setTimeout> | null = null;

/** Every library change ends in one of the events below; wait for the burst to end before re-asking. */
function scheduleInvalidate(): void {
  if (invalidateTimer !== null) clearTimeout(invalidateTimer);
  invalidateTimer = setTimeout(() => {
    invalidateTimer = null;
    useLibraryIndexStore.getState().invalidate();
  }, LIBRARY_DEBOUNCE_MS);
}

// Every library mutation (delete, trash restore, scan, edit, download) ends in a tabs or albums update.
useLibraryStore.subscribe((state, previous) => {
  if (state.tabs !== previous.tabs || state.albums !== previous.albums) {
    scheduleInvalidate();
  }
});

if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
  listen<{ task_id: string; message: string }>("download-task-completed", () =>
    scheduleInvalidate(),
  ).catch((e) => console.log(e));
}
