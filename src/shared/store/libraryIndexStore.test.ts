import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RemoteRef } from "@/shared/lib/library/libraryIndex.ts";
import type { Platform } from "@/shared/types.ts";
import {
  answersFor,
  CHUNK_SIZE,
  chunk,
  keepKeys,
  LIBRARY_DEBOUNCE_MS,
  REQUEST_WINDOW_MS,
  touchWatched,
  unseenRefs,
  WATCH_LIMIT,
} from "./libraryIndexStore.ts";

const { getDownloadStatusMock, getAlbumStatusMock, listenMock, libraryFake } = vi.hoisted(() => {
  type Listener = (state: Record<string, unknown>, previous: Record<string, unknown>) => void;
  const initial = (): Record<string, unknown> => ({
    tabs: [],
    albums: [],
    activeView: "ARTIST_ALBUMS",
  });
  let state = initial();
  let listeners = new Set<Listener>();
  // A minimal stand-in for the zustand library store; reset() drops the listeners of earlier store instances.
  const libraryFake = {
    getState: () => state,
    setState(patch: Record<string, unknown>) {
      const previous = state;
      state = { ...state, ...patch };
      for (const listener of [...listeners]) listener(state, previous);
    },
    subscribe(listener: Listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    reset() {
      state = initial();
      listeners = new Set();
    },
  };
  return {
    getDownloadStatusMock: vi.fn(),
    getAlbumStatusMock: vi.fn(),
    listenMock: vi.fn(),
    libraryFake,
  };
});

vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/library/libraryIndex.ts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/shared/lib/library/libraryIndex.ts")>()),
  getDownloadStatus: getDownloadStatusMock,
  getAlbumStatus: getAlbumStatusMock,
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({ useLibraryStore: libraryFake }));

const ref = (url: string, platform: Platform = "youtube"): RemoteRef => ({ platform, url });

/** Fresh module instance: the batcher keeps its queue and watched set per module instance. */
async function loadStore() {
  vi.resetModules();
  const indexModule = await import("./libraryIndexStore.ts");
  return { index: indexModule.useLibraryIndexStore, library: libraryFake };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  libraryFake.reset();
  getDownloadStatusMock.mockResolvedValue({});
  getAlbumStatusMock.mockResolvedValue({});
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("chunk", () => {
  it("splits into consecutive pieces of at most the size", () => {
    expect(chunk([1, 2, 3, 4, 5], 2)).toEqual([[1, 2], [3, 4], [5]]);
  });

  it("is empty for no items and never splits below one item", () => {
    expect(chunk([], 3)).toEqual([]);
    expect(chunk([1, 2], 0)).toEqual([[1], [2]]);
  });
});

describe("unseenRefs", () => {
  it("drops skipped keys and keeps the first of each duplicate, in order", () => {
    const out = unseenRefs(
      [ref("a"), ref("b"), ref("a"), ref("c")],
      (key) => key === "c",
    );
    expect(out.map((r) => r.url)).toEqual(["a", "b"]);
  });
});

describe("touchWatched", () => {
  it("moves a re-asked key to the most recent end", () => {
    const watched = new Map<string, RemoteRef>();
    touchWatched(watched, [ref("a"), ref("b")], 10);
    touchWatched(watched, [ref("a")], 10);
    expect([...watched.keys()]).toEqual(["b", "a"]);
  });

  it("drops the oldest keys beyond the limit", () => {
    const watched = new Map<string, RemoteRef>();
    touchWatched(watched, [ref("a"), ref("b"), ref("c")], 2);
    expect([...watched.keys()]).toEqual(["b", "c"]);
  });
});

describe("keepKeys", () => {
  it("keeps only the entries whose key is in the set", () => {
    expect(keepKeys({ a: "none", b: "downloaded" }, new Set(["b", "z"]))).toEqual({
      b: "downloaded",
    });
  });
});

describe("answersFor", () => {
  it("returns the answer per ref and the fallback for keys the backend left out", () => {
    const out = answersFor([ref("a"), ref("b")], { a: "downloaded" }, "none");
    expect(out).toEqual({ a: "downloaded", b: "none" });
  });

  it("uses the fallback for everything when there is no answer", () => {
    expect(answersFor([ref("a")], null, "none")).toEqual({ a: "none" });
  });
});

describe("library index store: batching", () => {
  it("sends one backend call for the refs requested in one window, without duplicates", async () => {
    const { index } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "downloaded", u2: "none", u3: "partial" });

    index.getState().requestTracks([ref("u1"), ref("u2")]);
    index.getState().requestTracks([ref("u3"), ref("u1")]);
    expect(getDownloadStatusMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(1);
    expect(getDownloadStatusMock.mock.calls[0][0].map((r: RemoteRef) => r.url)).toEqual([
      "u1",
      "u2",
      "u3",
    ]);
    expect(index.getState().tracks).toEqual({ u1: "downloaded", u2: "none", u3: "partial" });
  });

  it("does not ask again for cached urls", async () => {
    const { index } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "downloaded" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS * 4);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(1);
  });

  it("splits large batches into chunks of CHUNK_SIZE refs", async () => {
    const { index } = await loadStore();
    const refs = Array.from({ length: CHUNK_SIZE + 50 }, (_, i) => ref(`x${i}`));
    index.getState().requestTracks(refs);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    expect(getDownloadStatusMock).toHaveBeenCalledTimes(2);
    expect(getDownloadStatusMock.mock.calls[0][0]).toHaveLength(CHUNK_SIZE);
    expect(getDownloadStatusMock.mock.calls[1][0]).toHaveLength(50);
  });

  it("stores none for a url the backend did not answer, so it is not asked forever", async () => {
    const { index } = await loadStore();
    index.getState().requestTracks([ref("gone")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    expect(index.getState().tracks.gone).toBe("none");
  });

  it("keeps album answers apart and falls back to an empty status", async () => {
    const { index } = await loadStore();
    getAlbumStatusMock.mockResolvedValue({
      a1: { state: "partial", owned: 1, total: 3 },
    });
    index.getState().requestAlbums([ref("a1"), ref("a2")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    expect(getDownloadStatusMock).not.toHaveBeenCalled();
    expect(index.getState().albums).toEqual({
      a1: { state: "partial", owned: 1, total: 3 },
      a2: { state: "none", owned: 0, total: 0 },
    });
  });

  it("does not cache a failed request, so the next request asks again", async () => {
    const { index } = await loadStore();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    getDownloadStatusMock.mockRejectedValueOnce(new Error("db busy"));

    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(index.getState().tracks).toEqual({});
    expect(warn).toHaveBeenCalled();

    getDownloadStatusMock.mockResolvedValue({ u1: "downloaded" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(index.getState().tracks).toEqual({ u1: "downloaded" });
    warn.mockRestore();
  });
});

describe("library index store: invalidate", () => {
  it("re-asks every watched url in one batch and keeps the old values until the answers replace them", async () => {
    const { index } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "none", u2: "none" });
    index.getState().requestTracks([ref("u1"), ref("u2")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    getDownloadStatusMock.mockResolvedValue({ u1: "downloaded", u2: "none" });
    index.getState().invalidate();
    // No blink: the cached values stay visible while the refetch is in flight.
    expect(index.getState().tracks).toEqual({ u1: "none", u2: "none" });

    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(2);
    expect(getDownloadStatusMock.mock.calls[1][0].map((r: RemoteRef) => r.url)).toEqual([
      "u1",
      "u2",
    ]);
    expect(index.getState().tracks).toEqual({ u1: "downloaded", u2: "none" });
  });

  it("replaces a stale downloaded value with the new answer, including none", async () => {
    const { index } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "downloaded" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    getDownloadStatusMock.mockResolvedValue({ u1: "none" });
    index.getState().invalidate();
    expect(index.getState().tracks).toEqual({ u1: "downloaded" });

    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(index.getState().tracks).toEqual({ u1: "none" });
  });

  it("drops cached answers of urls that are no longer watched", async () => {
    const { index } = await loadStore();
    // WATCH_LIMIT + 1 urls: the first one falls out of the watched set.
    const refs = Array.from({ length: WATCH_LIMIT + 1 }, (_, i) => ref(`x${i}`));
    index.getState().requestTracks(refs);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(Object.keys(index.getState().tracks)).toHaveLength(WATCH_LIMIT + 1);

    index.getState().invalidate();
    const tracks = index.getState().tracks;
    expect(tracks).not.toHaveProperty("x0");
    expect(tracks).toHaveProperty("x1");
    expect(Object.keys(tracks)).toHaveLength(WATCH_LIMIT);
  });

  it("ignores an answer that was requested before the invalidate", async () => {
    const { index } = await loadStore();
    let resolveOld: (value: Record<string, string>) => void = () => {};
    getDownloadStatusMock.mockImplementationOnce(
      () => new Promise((resolve) => (resolveOld = resolve)),
    );
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    getDownloadStatusMock.mockResolvedValueOnce({ u1: "none" });
    index.getState().invalidate();
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(index.getState().tracks).toEqual({ u1: "none" });

    resolveOld({ u1: "downloaded" });
    await vi.advanceTimersByTimeAsync(0);
    expect(index.getState().tracks).toEqual({ u1: "none" });
  });

  it("runs after a library tabs or albums change once the burst is quiet", async () => {
    const { index, library } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "none" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(1);

    library.setState({ tabs: [{}] });
    await vi.advanceTimersByTimeAsync(LIBRARY_DEBOUNCE_MS - 1);
    library.setState({ albums: [{}] });
    await vi.advanceTimersByTimeAsync(LIBRARY_DEBOUNCE_MS - 1);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1 + REQUEST_WINDOW_MS);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(2);
  });

  it("does not run for other library state changes", async () => {
    const { index, library } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "none" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    library.setState({ activeView: "TRASH" });
    await vi.advanceTimersByTimeAsync(LIBRARY_DEBOUNCE_MS * 3);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(1);
  });

  it("runs when a download task completes", async () => {
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    listenMock.mockImplementation((name: string, handler: (event: { payload: unknown }) => void) => {
      handlers[name] = handler;
      return Promise.resolve(() => {});
    });

    const { index } = await loadStore();
    getDownloadStatusMock.mockResolvedValue({ u1: "none" });
    index.getState().requestTracks([ref("u1")]);
    await vi.advanceTimersByTimeAsync(REQUEST_WINDOW_MS);

    handlers["download-task-completed"]({ payload: { task_id: "t1", message: "done" } });
    await vi.advanceTimersByTimeAsync(LIBRARY_DEBOUNCE_MS + REQUEST_WINDOW_MS);
    expect(getDownloadStatusMock).toHaveBeenCalledTimes(2);
  });
});
