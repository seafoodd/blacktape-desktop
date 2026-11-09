import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  ItemType,
  useLibraryStore,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type {
  ArtistProfile,
  FullSearchPage,
  SearchKind,
} from "@/shared/lib/searchFull.ts";
import { useSearchPageStore } from "./searchPageStore.ts";
import { DEFAULT_FILTERS, DEFAULT_SORT } from "./searchPageFilters.ts";

const { searchFullMock, getArtistProfileMock } = vi.hoisted(() => ({
  searchFullMock: vi.fn(),
  getArtistProfileMock: vi.fn(),
}));

vi.mock("@/shared/lib/searchFull.ts", () => ({
  searchFull: searchFullMock,
  getArtistProfile: getArtistProfileMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@/shared/lib/search.ts", () => ({ searchPlatforms: vi.fn() }));

const initialState = useSearchPageStore.getState();

let counter = 0;
const item = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => {
  counter += 1;
  return {
    item_type: ItemType.Track,
    name: `Item ${counter}`,
    band_name: "Band",
    item_url_path: `https://example.test/${counter}`,
    img: "",
    platform: "youtube",
    ...over,
  };
};

const page = (
  items: SearchSuggestion[],
  over: Partial<FullSearchPage> = {},
): FullSearchPage => ({
  items,
  next_cursor: null,
  has_more: false,
  warnings: [],
  ...over,
});

const profileFixture = (name: string): ArtistProfile => ({
  platform: "youtube",
  name,
  url: "https://youtube.com/browse/UC1",
  avatar: null,
  banner: null,
  subscribers: 7_210_000,
  monthly_audience: null,
    followers: null,
  description: null,
  location: null,
  top_songs: [],
  albums: [],
  singles: [],
});

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

const store = () => useSearchPageStore.getState();
const section = (kind: SearchKind) => useSearchPageStore.getState().sections[kind];

const SEARCH_PLATFORMS = ["youtube", "bandcamp", "soundcloud"];

beforeEach(() => {
  searchFullMock.mockReset();
  getArtistProfileMock.mockReset();
  useSearchPageStore.setState(initialState, true);
  useLibraryStore.setState({ activeView: "ARTIST_ALBUMS" });
});

describe("open", () => {
  it("ignores a blank query: no request, no navigation", () => {
    searchFullMock.mockResolvedValue(page([]));
    store().open("   ");
    expect(searchFullMock).not.toHaveBeenCalled();
    expect(store().query).toBe("");
    expect(useLibraryStore.getState().activeView).toBe("ARTIST_ALBUMS");
  });

  it("trims the query, switches to the results view and starts three parallel searches", async () => {
    searchFullMock.mockResolvedValue(page([]));
    store().open("  daft punk  ");

    expect(store().query).toBe("daft punk");
    expect(useLibraryStore.getState().activeView).toBe("SEARCH_RESULTS");
    expect(searchFullMock).toHaveBeenCalledTimes(3);
    const kinds = searchFullMock.mock.calls.map((call) => call[0].kind).sort();
    expect(kinds).toEqual([ItemType.Album, ItemType.Artist, ItemType.Track].sort());
    for (const [args] of searchFullMock.mock.calls) {
      expect(args.query).toBe("daft punk");
      expect(args.platforms).toEqual(SEARCH_PLATFORMS);
    }
    // All three sections are loading right after open, before any response.
    expect(section(ItemType.Track).status).toBe("loading");
    expect(section(ItemType.Album).status).toBe("loading");
    expect(section(ItemType.Artist).status).toBe("loading");

    await flush();
  });

  it("a different query resets the filters but keeps the selected tab", async () => {
    searchFullMock.mockResolvedValue(page([]));
    store().open("daft punk");
    await flush();

    store().setFilters({
      tab: ItemType.Album,
      platform: "youtube",
      year: { min: 2000, max: 2010 },
      durationSecs: { max: 300 },
      text: "homework",
    });

    store().open("radiohead");
    await flush();

    expect(store().query).toBe("radiohead");
    expect(store().filters).toEqual({ ...DEFAULT_FILTERS, tab: ItemType.Album });
    await flush();
  });

  it("the same query keeps the filters untouched", async () => {
    searchFullMock.mockResolvedValue(page([]));
    store().open("daft punk");
    await flush();

    const chosen = {
      tab: ItemType.Artist as const,
      platform: "bandcamp" as const,
      year: { min: 1997 },
      durationSecs: { min: 60, max: 600 },
      text: "discovery",
    };
    store().setFilters(chosen);
    const before = store().filters;

    store().open("  daft punk  ");
    await flush();

    expect(store().query).toBe("daft punk");
    expect(store().filters).toBe(before);
    expect(store().filters).toEqual(chosen);
    await flush();
  });
});

describe("first page results", () => {
  it("stores items, cursor, has_more and warning messages per section", async () => {
    const t1 = item({ item_type: ItemType.Track });
    const t2 = item({ item_type: ItemType.Track });
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Track) {
        return page([t1, t2], {
          next_cursor: { youtube: "tok-1" },
          has_more: true,
          warnings: [{ platform: "bandcamp", message: "Bandcamp unavailable" }],
        });
      }
      return page([]);
    });

    store().open("daft punk");
    await flush();

    const tracks = section(ItemType.Track);
    expect(tracks.status).toBe("ready");
    expect(tracks.items).toEqual([t1, t2]);
    expect(tracks.hasMore).toBe(true);
    expect(tracks.nextCursor).toEqual({ youtube: "tok-1" });
    expect(tracks.warnings).toEqual(["Bandcamp unavailable"]);
    expect(section(ItemType.Album).status).toBe("ready");
    expect(section(ItemType.Album).hasMore).toBe(false);
    expect(section(ItemType.Album).nextCursor).toBeNull();
  });

  it("treats has_more without a cursor as no more pages", async () => {
    searchFullMock.mockResolvedValue(page([item()], { has_more: true, next_cursor: null }));
    store().open("x y");
    await flush();
    expect(section(ItemType.Track).hasMore).toBe(false);
  });

  it("a failed section does not affect the others", async () => {
    const track = item({ item_type: ItemType.Track });
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Album) throw "boom";
      return page(kind === ItemType.Track ? [track] : []);
    });

    store().open("daft punk");
    await flush();

    expect(section(ItemType.Album).status).toBe("error");
    expect(section(ItemType.Album).error).toBe("boom");
    expect(section(ItemType.Album).items).toEqual([]);
    expect(section(ItemType.Track).status).toBe("ready");
    expect(section(ItemType.Track).items).toEqual([track]);
  });

  it("reads the message of an Error rejection and falls back for unknown errors", async () => {
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Track) throw new Error("kaput");
      if (kind === ItemType.Album) throw { code: 1 };
      return page([]);
    });

    store().open("daft punk");
    await flush();

    expect(section(ItemType.Track).error).toBe("kaput");
    expect(section(ItemType.Album).error).toBe("Search failed");
  });

  it("ignores the response of an older query when a newer search has started", async () => {
    const pending: Record<string, Deferred<FullSearchPage>> = {};
    searchFullMock.mockImplementation(({ query, kind }: { query: string; kind: SearchKind }) => {
      const d = deferred<FullSearchPage>();
      pending[`${query}|${kind}`] = d;
      return d.promise;
    });

    store().open("old");
    store().open("new");

    const fresh = item({ name: "fresh" });
    const stale = item({ name: "stale" });
    pending["new|Track"].resolve(page([fresh]));
    await flush();
    pending["old|Track"].resolve(page([stale]));
    await flush();

    expect(store().query).toBe("new");
    expect(section(ItemType.Track).items).toEqual([fresh]);
    expect(section(ItemType.Track).status).toBe("ready");

    for (const kind of [ItemType.Album, ItemType.Artist]) {
      pending[`new|${kind}`].resolve(page([]));
      pending[`old|${kind}`].resolve(page([]));
    }
    await flush();
  });
});

describe("profile", () => {
  it("fetches the profile of the matching artist after the Artist section is ready", async () => {
    const artist = item({
      item_type: ItemType.Artist,
      name: "Daft Punk",
      item_url_path: "https://youtube.com/browse/UC1",
      platform: "youtube",
    });
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) =>
      page(kind === ItemType.Artist ? [artist] : []),
    );
    const profile = profileFixture("Daft Punk");
    getArtistProfileMock.mockResolvedValue(profile);

    store().open("daft punk");
    await flush();

    expect(getArtistProfileMock).toHaveBeenCalledWith("youtube", "https://youtube.com/browse/UC1");
    expect(store().profile.status).toBe("ready");
    expect(store().profile.data).toEqual(profile);
    expect(store().profile.artist).toEqual(artist);
  });

  it("does not fetch a profile when no artist matches the query", async () => {
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) =>
      page(kind === ItemType.Artist ? [item({ item_type: ItemType.Artist, name: "Someone Else" })] : []),
    );

    store().open("daft punk");
    await flush();

    expect(getArtistProfileMock).not.toHaveBeenCalled();
    expect(store().profile.status).toBe("idle");
    expect(store().profile.artist).toBeNull();
  });

  it("a failed profile request leaves status error and no data", async () => {
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) =>
      page(kind === ItemType.Artist ? [item({ item_type: ItemType.Artist, name: "Daft Punk" })] : []),
    );
    getArtistProfileMock.mockRejectedValue("profile unavailable");

    store().open("daft punk");
    await flush();

    expect(store().profile.status).toBe("error");
    expect(store().profile.data).toBeNull();
    expect(section(ItemType.Artist).status).toBe("ready");
  });

  it("ignores a profile that resolves after a newer search started", async () => {
    const artistPage = page([item({ item_type: ItemType.Artist, name: "Daft Punk" })]);
    searchFullMock.mockImplementation(async ({ query, kind }: { query: string; kind: SearchKind }) => {
      if (query === "daft punk" && kind === ItemType.Artist) return artistPage;
      return page([]);
    });
    const slowProfile = deferred<ArtistProfile>();
    getArtistProfileMock.mockReturnValue(slowProfile.promise);

    store().open("daft punk");
    await flush();
    expect(store().profile.status).toBe("loading");

    store().open("other query");
    await flush();
    slowProfile.resolve(profileFixture("Daft Punk"));
    await flush();

    expect(store().query).toBe("other query");
    expect(store().profile.status).toBe("idle");
    expect(store().profile.data).toBeNull();
  });
});

describe("loadMore", () => {
  const firstPageWithCursor = async (tracks: SearchSuggestion[]) => {
    searchFullMock.mockImplementation(async ({ kind, cursor }: { kind: SearchKind; cursor?: unknown }) => {
      if (kind === ItemType.Track && !cursor) {
        return page(tracks, { next_cursor: { youtube: "c1" }, has_more: true });
      }
      return page([]);
    });
    store().open("daft punk");
    await flush();
  };

  it("requests the next page with the cursor and appends without duplicates", async () => {
    const t1 = item({ name: "t1" });
    const t2 = item({ name: "t2" });
    const t3 = item({ name: "t3" });
    await firstPageWithCursor([t1, t2]);

    const more = deferred<FullSearchPage>();
    searchFullMock.mockImplementationOnce(() => more.promise);

    const pendingLoad = store().loadMore(ItemType.Track);
    expect(section(ItemType.Track).status).toBe("loadingMore");
    expect(searchFullMock).toHaveBeenLastCalledWith({
      query: "daft punk",
      kind: ItemType.Track,
      platforms: SEARCH_PLATFORMS,
      cursor: { youtube: "c1" },
    });

    more.resolve(page([t2, t3], { has_more: false, next_cursor: null }));
    await pendingLoad;

    const tracks = section(ItemType.Track);
    expect(tracks.items).toEqual([t1, t2, t3]);
    expect(tracks.status).toBe("ready");
    expect(tracks.hasMore).toBe(false);
    expect(tracks.nextCursor).toBeNull();
  });

  it("is a no-op while a section is loading, after the last page, or when already loading more", async () => {
    // Loading: no request.
    searchFullMock.mockReturnValue(new Promise(() => {}));
    store().open("daft punk");
    await store().loadMore(ItemType.Track);
    expect(searchFullMock).toHaveBeenCalledTimes(3);

    // Last page: no request.
    await firstPageWithCursor([]);
    searchFullMock.mockReset();
    await store().loadMore(ItemType.Album);
    expect(searchFullMock).not.toHaveBeenCalled();
  });

  it("does not start a second request while one is in flight", async () => {
    await firstPageWithCursor([item()]);
    searchFullMock.mockReset();
    const more = deferred<FullSearchPage>();
    searchFullMock.mockReturnValue(more.promise);

    const first = store().loadMore(ItemType.Track);
    await store().loadMore(ItemType.Track);
    expect(searchFullMock).toHaveBeenCalledTimes(1);

    more.resolve(page([]));
    await first;
  });

  it("is a no-op for a section that failed on the first page", async () => {
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Album) throw "nope";
      return page([]);
    });
    store().open("daft punk");
    await flush();
    searchFullMock.mockClear();

    await store().loadMore(ItemType.Album);
    expect(searchFullMock).not.toHaveBeenCalled();
    expect(section(ItemType.Album).status).toBe("error");
  });

  it("a failed request keeps the items, shows the error and can be retried", async () => {
    const t1 = item({ name: "t1" });
    await firstPageWithCursor([t1]);
    searchFullMock.mockRejectedValueOnce("network down");

    await store().loadMore(ItemType.Track);
    let tracks = section(ItemType.Track);
    expect(tracks.status).toBe("ready");
    expect(tracks.items).toEqual([t1]);
    expect(tracks.error).toBe("network down");
    expect(tracks.hasMore).toBe(true);

    const t2 = item({ name: "t2" });
    searchFullMock.mockResolvedValueOnce(page([t2]));
    await store().loadMore(ItemType.Track);
    tracks = section(ItemType.Track);
    expect(tracks.items).toEqual([t1, t2]);
    expect(tracks.error).toBeNull();
  });

  it("keeps the first page warnings and merges new ones without duplicates", async () => {
    searchFullMock.mockImplementation(async ({ kind, cursor }: { kind: SearchKind; cursor?: unknown }) => {
      if (kind === ItemType.Track && !cursor) {
        return page([item({ name: "t1" })], {
          next_cursor: { youtube: "c1" },
          has_more: true,
          warnings: [{ platform: "bandcamp", message: "A" }],
        });
      }
      return page([]);
    });
    store().open("daft punk");
    await flush();
    expect(section(ItemType.Track).warnings).toEqual(["A"]);

    searchFullMock.mockResolvedValueOnce(
      page([item({ name: "t2" })], {
        warnings: [
          { platform: "bandcamp", message: "A" },
          { platform: "youtube", message: "B" },
        ],
      }),
    );
    await store().loadMore(ItemType.Track);

    expect(section(ItemType.Track).warnings).toEqual(["A", "B"]);
    expect(section(ItemType.Track).items).toHaveLength(2);
  });

  it("drops a slow page that arrives after a new search started", async () => {
    const t1 = item({ name: "t1" });
    await firstPageWithCursor([t1]);

    const slowMore = deferred<FullSearchPage>();
    searchFullMock.mockImplementationOnce(() => slowMore.promise);
    const pendingLoad = store().loadMore(ItemType.Track);

    const fresh = item({ name: "fresh" });
    searchFullMock.mockImplementation(async ({ kind }: { kind: SearchKind }) =>
      page(kind === ItemType.Track ? [fresh] : []),
    );
    store().open("another");
    await flush();

    slowMore.resolve(page([item({ name: "stale-more" })]));
    await pendingLoad;

    expect(store().query).toBe("another");
    expect(section(ItemType.Track).items).toEqual([fresh]);
  });
});

describe("retry", () => {
  it("re-runs the whole search for the current query", async () => {
    searchFullMock.mockResolvedValue(page([]));
    store().open("daft punk");
    await flush();
    searchFullMock.mockClear();

    store().retry();
    expect(searchFullMock).toHaveBeenCalledTimes(3);
    for (const [args] of searchFullMock.mock.calls) {
      expect(args.query).toBe("daft punk");
    }
    await flush();
  });

  it("does nothing without a query", () => {
    store().retry();
    expect(searchFullMock).not.toHaveBeenCalled();
  });
});

describe("filters and sort", () => {
  it("setFilters merges the patch into the current filters", () => {
    store().setFilters({ platform: "youtube" });
    store().setFilters({ text: "x" });
    expect(store().filters).toEqual({ ...DEFAULT_FILTERS, platform: "youtube", text: "x" });
  });

  it("resetFilters restores the defaults", () => {
    store().setFilters({ platform: "bandcamp", year: { min: 2000 }, text: "abc" });
    store().resetFilters();
    expect(store().filters).toEqual(DEFAULT_FILTERS);
  });

  it("setSort replaces the sort", () => {
    store().setSort({ key: "year", dir: "desc" });
    expect(store().sort).toEqual({ key: "year", dir: "desc" });
    store().setSort(DEFAULT_SORT);
    expect(store().sort).toEqual(DEFAULT_SORT);
  });
});
