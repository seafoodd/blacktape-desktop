import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ItemType, useLibraryStore, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { FullSearchPage, SearchKind } from "@/shared/lib/search/searchFull.ts";
import { useSearchPageStore } from "./searchPageStore.ts";

type Handler = (event: { payload: unknown }) => void;

const { searchFullMock, getArtistProfileMock, listenMock, listeners } = vi.hoisted(() => {
  const listeners = new Map<string, Set<Handler>>();
  const listenMock = vi.fn(async (event: string, handler: Handler) => {
    const handlers = listeners.get(event) ?? new Set<Handler>();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => handlers.delete(handler);
  });
  return { searchFullMock: vi.fn(), getArtistProfileMock: vi.fn(), listenMock, listeners };
});

vi.mock("@/shared/lib/search/searchFull.ts", () => ({
  searchFull: searchFullMock,
  getArtistProfile: getArtistProfileMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({ getArtistAlbums: vi.fn(), getArtists: vi.fn() }));
vi.mock("@/shared/lib/search/search.ts", () => ({ searchPlatforms: vi.fn() }));

function item(name: string, platform: "bandcamp" | "soundcloud"): SearchSuggestion {
  return {
    item_type: ItemType.Track,
    name,
    band_name: "Artist",
    item_url_path: `https://${platform}.example/${name}`,
    img: "",
    platform,
  };
}

function page(
  items: SearchSuggestion[],
  warnings: FullSearchPage["warnings"] = [],
): FullSearchPage {
  return { items, copies: [], next_cursor: null, has_more: false, warnings };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

function emitProgress(requestId: number, source: "bandcamp" | "soundcloud", state: string) {
  for (const handler of [...(listeners.get("search-full-progress") ?? [])]) {
    handler({ payload: { requestId, progress: { source, state, elapsedMs: 1, count: 1 } } });
  }
}

beforeEach(() => {
  searchFullMock.mockReset();
  getArtistProfileMock.mockReset();
  listeners.clear();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  useSearchPageStore.setState({ ...useSearchPageStore.getState(), query: "" });
  useLibraryStore.setState({ activeView: "ARTIST_ALBUMS" });
});

afterEach(() => vi.unstubAllGlobals());

describe("search page source retry", () => {
  it("retries only failed SoundCloud and preserves healthy rows while merging retry results", async () => {
    const initialTrack = deferred<FullSearchPage>();
    searchFullMock.mockImplementation(({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Track) return initialTrack.promise;
      return Promise.resolve(page([]));
    });
    useSearchPageStore.getState().open("radiohead");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(3));
    const trackCall = searchFullMock.mock.calls.find(([args]) => args.kind === ItemType.Track);
    const initialId: number = trackCall?.[0].requestId;
    emitProgress(initialId, "bandcamp", "done");
    emitProgress(initialId, "soundcloud", "failed");
    initialTrack.resolve(page([item("Bandcamp result", "bandcamp")], [{
      platform: "soundcloud",
      source: "soundcloud",
      message: "request timed out",
    }]));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));

    const retry = deferred<FullSearchPage>();
    searchFullMock.mockImplementationOnce(() => retry.promise);
    const retryRun = useSearchPageStore.getState().retrySource(ItemType.Track, "soundcloud");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(4));
    const [retryArgs] = searchFullMock.mock.calls[3];
    expect(retryArgs.sources).toEqual(["soundcloud"]);
    expect(useSearchPageStore.getState().sections[ItemType.Track].items).toEqual([
      item("Bandcamp result", "bandcamp"),
    ]);

    await useSearchPageStore.getState().retrySource(ItemType.Track, "soundcloud");
    await useSearchPageStore.getState().retrySource(ItemType.Track, "bandcamp");
    expect(searchFullMock).toHaveBeenCalledTimes(4);

    emitProgress(retryArgs.requestId, "soundcloud", "done");
    retry.resolve(page([item("SoundCloud result", "soundcloud")]));
    await retryRun;

    const section = useSearchPageStore.getState().sections[ItemType.Track];
    expect(section.items.map((entry) => entry.name).sort()).toEqual(["Bandcamp result", "SoundCloud result"]);
    expect(section.progress).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "bandcamp", state: "done" }),
      expect.objectContaining({ source: "soundcloud", state: "done" }),
    ]));
  });
});
