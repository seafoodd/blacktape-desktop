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

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

const item = (name: string): SearchSuggestion => ({
  item_type: ItemType.Track,
  name,
  band_name: "Artist",
  item_url_path: `https://example.test/${name}`,
  img: "",
  platform: "youtube",
});

const page = (items: SearchSuggestion[]): FullSearchPage => ({
  items,
  copies: [],
  next_cursor: null,
  has_more: false,
  warnings: [],
});

function emit(event: string, payload: unknown) {
  for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
}

async function waitForRequests(count: number) {
  await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(count));
}

function requestId(query: string, kind: SearchKind): number {
  const call = searchFullMock.mock.calls.find(([args]) => args.query === query && args.kind === kind);
  if (!call) throw new Error(`missing request for ${query} ${kind}`);
  return call[0].requestId;
}

function resolveQuery(query: string, values: Map<string, Deferred<FullSearchPage>>, result: (kind: SearchKind) => FullSearchPage) {
  for (const kind of [ItemType.Track, ItemType.Album, ItemType.Artist] as SearchKind[]) {
    values.get(`${query}:${kind}`)?.resolve(result(kind));
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

describe("search full partial events", () => {
  it("fills the matching section while keeping it loading until the final response", async () => {
    const pending = new Map<string, Deferred<FullSearchPage>>();
    searchFullMock.mockImplementation(({ query, kind }: { query: string; kind: SearchKind }) => {
      const task = deferred<FullSearchPage>();
      pending.set(`${query}:${kind}`, task);
      return task.promise;
    });
    useSearchPageStore.getState().open("radiohead");
    await waitForRequests(3);

    const id = requestId("radiohead", ItemType.Track);
    const partialPage = page([item("early track")]);
    emit("search-full-partial", { requestId: id, kind: ItemType.Track, page: partialPage });

    expect(useSearchPageStore.getState().sections[ItemType.Track].items).toEqual(partialPage.items);
    expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("loading");

    resolveQuery("radiohead", pending, () => page([]));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));
  });

  it("ignores partial events with a foreign request id", async () => {
    const pending = new Map<string, Deferred<FullSearchPage>>();
    searchFullMock.mockImplementation(({ query, kind }: { query: string; kind: SearchKind }) => {
      const task = deferred<FullSearchPage>();
      pending.set(`${query}:${kind}`, task);
      return task.promise;
    });
    useSearchPageStore.getState().open("radiohead");
    await waitForRequests(3);

    emit("search-full-partial", {
      requestId: requestId("radiohead", ItemType.Track) + 999,
      kind: ItemType.Track,
      page: page([item("foreign")]),
    });
    expect(useSearchPageStore.getState().sections[ItemType.Track].items).toEqual([]);

    resolveQuery("radiohead", pending, () => page([]));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));
  });

  it("ignores an old query partial after a newer query starts", async () => {
    const pending = new Map<string, Deferred<FullSearchPage>>();
    searchFullMock.mockImplementation(({ query, kind }: { query: string; kind: SearchKind }) => {
      const task = deferred<FullSearchPage>();
      pending.set(`${query}:${kind}`, task);
      return task.promise;
    });
    useSearchPageStore.getState().open("old query");
    await waitForRequests(3);
    useSearchPageStore.getState().open("new query");
    await waitForRequests(6);

    emit("search-full-partial", {
      requestId: requestId("old query", ItemType.Track),
      kind: ItemType.Track,
      page: page([item("stale")]),
    });
    expect(useSearchPageStore.getState().query).toBe("new query");
    expect(useSearchPageStore.getState().sections[ItemType.Track].items).toEqual([]);

    resolveQuery("old query", pending, () => page([]));
    resolveQuery("new query", pending, () => page([]));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));
  });

  it("ignores a partial event after the final section response", async () => {
    const pending = new Map<string, Deferred<FullSearchPage>>();
    searchFullMock.mockImplementation(({ query, kind }: { query: string; kind: SearchKind }) => {
      const task = deferred<FullSearchPage>();
      pending.set(`${query}:${kind}`, task);
      return task.promise;
    });
    useSearchPageStore.getState().open("radiohead");
    await waitForRequests(3);

    const final = page([item("final track")]);
    pending.get(`radiohead:${ItemType.Track}`)?.resolve(final);
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));
    emit("search-full-partial", {
      requestId: requestId("radiohead", ItemType.Track),
      kind: ItemType.Track,
      page: page([item("late partial")]),
    });
    expect(useSearchPageStore.getState().sections[ItemType.Track].items).toEqual(final.items);

    resolveQuery("radiohead", pending, () => page([]));
  });
});
