import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";

type Handler = (event: { payload: unknown }) => void;

const { searchPlatformsMock, listenMock, listeners } = vi.hoisted(() => {
  const listeners = new Map<string, Set<Handler>>();
  const listenMock = vi.fn(async (event: string, handler: Handler) => {
    const handlers = listeners.get(event) ?? new Set<Handler>();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => handlers.delete(handler);
  });
  return { searchPlatformsMock: vi.fn(), listenMock, listeners };
});

vi.mock("@/shared/lib/search/search.ts", () => ({ searchPlatforms: searchPlatformsMock }));
vi.mock("@/shared/lib/audio.ts", () => ({ getArtistAlbums: vi.fn(), getArtists: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (path: string) => path }));

const item = (name: string) => ({ name }) as unknown as SearchSuggestion;

function emit(event: string, payload: unknown) {
  for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
}

function emitProgress(requestId: number, source: "youtube" | "bandcamp", state: string) {
  emit("search-progress", {
    requestId,
    progress: { source, state, elapsedMs: 1, count: 0 },
  });
}

async function startSearch(query: string) {
  const callCount = searchPlatformsMock.mock.calls.length + 1;
  const run = useLibraryStore.getState().executeSearch(query);
  await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalledTimes(callCount));
  const requestId: number = searchPlatformsMock.mock.calls[callCount - 1][2];
  return { run, requestId };
}

describe("executeSearch source-result cache", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listeners.clear();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    useLibraryStore.setState({
      searchQuery: "",
      searchResults: [],
      searchCache: {},
      searchCacheExpires: {},
      searchWarning: null,
      partialResultsFor: null,
      searchRun: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("does not cache results when a source failed", async () => {
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementationOnce(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );
    const { run, requestId } = await startSearch("Radiohead");
    emitProgress(requestId, "youtube", "done");
    emitProgress(requestId, "bandcamp", "failed");
    finish([item("result")]);
    await run;

    expect(useLibraryStore.getState().searchCache).not.toHaveProperty("radiohead");
    expect(useLibraryStore.getState().searchCacheExpires).not.toHaveProperty("radiohead");

    searchPlatformsMock.mockResolvedValueOnce([]);
    await useLibraryStore.getState().executeSearch("radiohead");
    expect(searchPlatformsMock).toHaveBeenCalledTimes(2);
  });

  it("caches all-done runs for five minutes, then fetches after expiry", async () => {
    const clock = vi.spyOn(Date, "now").mockReturnValue(1_000_000);
    let finish: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementationOnce(
      () => new Promise<SearchSuggestion[]>((resolve) => (finish = resolve)),
    );
    const { run, requestId } = await startSearch("Radiohead");
    emitProgress(requestId, "youtube", "done");
    emitProgress(requestId, "bandcamp", "done");
    finish([item("cached")]);
    await run;

    expect(useLibraryStore.getState().searchCache.radiohead).toEqual([item("cached")]);
    expect(useLibraryStore.getState().searchCacheExpires.radiohead).toBe(1_300_000);

    searchPlatformsMock.mockClear();
    await useLibraryStore.getState().executeSearch(" RADIOHEAD ");
    expect(searchPlatformsMock).not.toHaveBeenCalled();

    clock.mockReturnValue(1_300_001);
    let finishRefresh: (value: SearchSuggestion[]) => void = () => {};
    searchPlatformsMock.mockImplementationOnce(
      () => new Promise<SearchSuggestion[]>((resolve) => (finishRefresh = resolve)),
    );
    const { run: refreshed, requestId: refreshedId } = await startSearch("radiohead");
    emitProgress(refreshedId, "youtube", "done");
    emitProgress(refreshedId, "bandcamp", "done");
    finishRefresh([item("refreshed")]);
    await refreshed;
    expect(searchPlatformsMock).toHaveBeenCalledTimes(1);
    expect(useLibraryStore.getState().searchCache.radiohead).toEqual([item("refreshed")]);
  });

  it("does not let a late run with an incomplete source populate the cache", async () => {
    const finishes: Array<(value: SearchSuggestion[]) => void> = [];
    searchPlatformsMock.mockImplementation(
      () => new Promise<SearchSuggestion[]>((resolve) => finishes.push(resolve)),
    );

    const older = await startSearch("Radiohead");
    const newer = await startSearch("Blur");
    emitProgress(older.requestId, "youtube", "failed");
    emitProgress(newer.requestId, "youtube", "done");
    emitProgress(newer.requestId, "bandcamp", "done");

    finishes[1]([item("newer")]);
    await newer.run;
    finishes[0]([item("older")]);
    await older.run;

    expect(useLibraryStore.getState().searchCache).toHaveProperty("blur");
    expect(useLibraryStore.getState().searchCache).not.toHaveProperty("radiohead");
  });
});
