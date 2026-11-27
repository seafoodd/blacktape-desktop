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

const item = (name: string, platform: "bandcamp" | "soundcloud"): SearchSuggestion => ({
  item_type: "Track" as SearchSuggestion["item_type"],
  name,
  band_name: "Artist",
  item_url_path: `https://${platform}.example/${name}`,
  img: "",
  platform,
});

function emit(requestId: number, source: "bandcamp" | "soundcloud", state: string) {
  for (const handler of [...(listeners.get("search-progress") ?? [])]) {
    handler({ payload: { requestId, progress: { source, state, elapsedMs: 1, count: 1 } } });
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

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

afterEach(() => vi.unstubAllGlobals());

describe("library source retry", () => {
  it("retries only the failed source and merges its rows with healthy results", async () => {
    const initial = deferred<SearchSuggestion[]>();
    const retry = deferred<SearchSuggestion[]>();
    searchPlatformsMock.mockReturnValueOnce(initial.promise).mockReturnValueOnce(retry.promise);
    const firstRun = useLibraryStore.getState().executeSearch("Radiohead");
    await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalledTimes(1));
    const firstId = searchPlatformsMock.mock.calls[0][2];
    emit(firstId, "bandcamp", "done");
    emit(firstId, "soundcloud", "failed");
    initial.resolve([item("Bandcamp result", "bandcamp")]);
    await firstRun;

    const retryRun = useLibraryStore.getState().retrySearchSource("soundcloud");
    await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalledTimes(2));
    const [query, _platforms, retryId, sources] = searchPlatformsMock.mock.calls[1];
    expect(query).toBe("radiohead");
    expect(sources).toEqual(["soundcloud"]);
    expect(useLibraryStore.getState().searchResults).toEqual([item("Bandcamp result", "bandcamp")]);

    emit(retryId, "soundcloud", "done");
    retry.resolve([item("SoundCloud result", "soundcloud")]);
    await retryRun;

    expect(useLibraryStore.getState().searchResults.map((result) => result.name).sort()).toEqual([
      "Bandcamp result",
      "SoundCloud result",
    ]);
    expect(useLibraryStore.getState().searchRun?.sources).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "bandcamp", state: "done" }),
      expect.objectContaining({ source: "soundcloud", state: "done" }),
    ]));
  });

  it("ignores retries while a source request is active or after it is healthy", async () => {
    const initial = deferred<SearchSuggestion[]>();
    searchPlatformsMock.mockReturnValueOnce(initial.promise);
    const firstRun = useLibraryStore.getState().executeSearch("Radiohead");
    await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalledTimes(1));
    const firstId = searchPlatformsMock.mock.calls[0][2];
    emit(firstId, "bandcamp", "done");
    emit(firstId, "soundcloud", "failed");
    initial.resolve([item("Bandcamp result", "bandcamp")]);
    await firstRun;

    const retry = deferred<SearchSuggestion[]>();
    searchPlatformsMock.mockReturnValueOnce(retry.promise);
    const activeRetry = useLibraryStore.getState().retrySearchSource("soundcloud");
    await vi.waitFor(() => expect(searchPlatformsMock).toHaveBeenCalledTimes(2));
    await useLibraryStore.getState().retrySearchSource("soundcloud");
    await useLibraryStore.getState().retrySearchSource("bandcamp");
    expect(searchPlatformsMock).toHaveBeenCalledTimes(2);

    const retryId = searchPlatformsMock.mock.calls[1][2];
    emit(retryId, "soundcloud", "done");
    retry.resolve([item("SoundCloud result", "soundcloud")]);
    await activeRetry;
  });
});
