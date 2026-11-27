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

function item(name: string, platform: "youtube" | "bandcamp" | "soundcloud"): SearchSuggestion {
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
  nextCursor: FullSearchPage["next_cursor"] = null,
  warnings: FullSearchPage["warnings"] = [],
): FullSearchPage {
  return { items, copies: [], next_cursor: nextCursor, has_more: nextCursor !== null, warnings };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

function emit(requestId: number, source: "youtube" | "bandcamp" | "soundcloud" | "musicbrainz", state: string) {
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

describe("search page continuation retry", () => {
  it("preserves nonpaged failures and retries only SoundCloud from its failed cursor", async () => {
    const initialTrack = deferred<FullSearchPage>();
    const moreTrack = deferred<FullSearchPage>();
    const retrySoundcloud = deferred<FullSearchPage>();
    searchFullMock.mockImplementation(({ kind, cursor, sources }: {
      kind: SearchKind;
      cursor?: FullSearchPage["next_cursor"];
      sources?: string[];
    }) => {
      if (kind !== ItemType.Track) return Promise.resolve(page([]));
      if (sources?.includes("soundcloud")) return retrySoundcloud.promise;
      if (cursor) return moreTrack.promise;
      return initialTrack.promise;
    });

    useSearchPageStore.getState().open("radiohead");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(3));
    const firstTrackArgs = searchFullMock.mock.calls.find(([args]) => args.kind === ItemType.Track)?.[0];
    if (!firstTrackArgs) throw new Error("missing first-page track request");
    const firstRequestId: number = firstTrackArgs.requestId;
    emit(firstRequestId, "bandcamp", "failed");
    emit(firstRequestId, "musicbrainz", "failed");
    emit(firstRequestId, "youtube", "done");
    emit(firstRequestId, "soundcloud", "done");
    initialTrack.resolve(page(
      [item("Initial YouTube", "youtube")],
      { youtube: "yt-old", soundcloud: 40 },
      [
        { platform: "bandcamp", source: "bandcamp", message: "Bandcamp unavailable" },
        { platform: "local", source: "musicbrainz", message: "MusicBrainz unavailable" },
      ],
    ));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));

    const loadMore = useSearchPageStore.getState().loadMore(ItemType.Track);
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(4));
    const moreArgs = searchFullMock.mock.calls[3][0];
    expect(moreArgs.cursor).toEqual({ youtube: "yt-old", soundcloud: 40 });
    emit(moreArgs.requestId, "youtube", "running");
    emit(moreArgs.requestId, "youtube", "done");
    emit(moreArgs.requestId, "soundcloud", "failed");
    moreTrack.resolve(page(
      [item("Next YouTube", "youtube")],
      { youtube: "yt-next", soundcloud: null },
      [{ platform: "soundcloud", source: "soundcloud", message: "SoundCloud unavailable" }],
    ));
    await loadMore;

    const afterMore = useSearchPageStore.getState().sections[ItemType.Track];
    expect(afterMore.progress).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "bandcamp", state: "failed" }),
      expect.objectContaining({ source: "musicbrainz", state: "failed" }),
      expect.objectContaining({ source: "soundcloud", state: "failed" }),
    ]));
    expect(afterMore.nextCursor).toEqual({ youtube: "yt-next", soundcloud: 40 });

    const retry = useSearchPageStore.getState().retrySource(ItemType.Track, "soundcloud");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(5));
    const retryArgs = searchFullMock.mock.calls[4][0];
    expect(retryArgs.sources).toEqual(["soundcloud"]);
    expect(retryArgs.cursor).toEqual(expect.objectContaining({ soundcloud: 40 }));
    emit(retryArgs.requestId, "soundcloud", "running");
    emit(retryArgs.requestId, "soundcloud", "done");
    retrySoundcloud.resolve(page(
      [item("Recovered SoundCloud", "soundcloud")],
      { youtube: "yt-next", soundcloud: 60 },
    ));
    await retry;

    const finalSection = useSearchPageStore.getState().sections[ItemType.Track];
    expect(finalSection.items.map((entry) => entry.name).sort()).toEqual([
      "Initial YouTube",
      "Next YouTube",
      "Recovered SoundCloud",
    ]);
    expect(finalSection.nextCursor).toEqual({ youtube: "yt-next", soundcloud: 60 });
    expect(finalSection.progress).toEqual(expect.arrayContaining([
      expect.objectContaining({ source: "bandcamp", state: "failed" }),
      expect.objectContaining({ source: "musicbrainz", state: "failed" }),
      expect.objectContaining({ source: "soundcloud", state: "done" }),
    ]));
  });

  it("retries a failed first-page YouTube chain from the token that failed", async () => {
    const initialTrack = deferred<FullSearchPage>();
    const retryYoutube = deferred<FullSearchPage>();
    searchFullMock.mockImplementation(({ kind, sources }: {
      kind: SearchKind;
      sources?: string[];
    }) => {
      if (kind !== ItemType.Track) return Promise.resolve(page([]));
      if (sources?.includes("youtube")) return retryYoutube.promise;
      return initialTrack.promise;
    });

    useSearchPageStore.getState().open("radiohead");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(3));
    const firstTrackArgs = searchFullMock.mock.calls.find(([args]) => args.kind === ItemType.Track)?.[0];
    emit(firstTrackArgs.requestId, "youtube", "failed");
    emit(firstTrackArgs.requestId, "bandcamp", "done");
    emit(firstTrackArgs.requestId, "soundcloud", "done");
    initialTrack.resolve(page(
      [item("First page result", "youtube")],
      { youtube: "failed-first-chain-token", soundcloud: 20 },
      [{ platform: "youtube", source: "youtube", message: "continuation timed out" }],
    ));
    await vi.waitFor(() => expect(useSearchPageStore.getState().sections[ItemType.Track].status).toBe("ready"));

    const retry = useSearchPageStore.getState().retrySource(ItemType.Track, "youtube");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(4));
    const retryArgs = searchFullMock.mock.calls[3][0];
    expect(retryArgs.sources).toEqual(["youtube"]);
    expect(retryArgs.cursor).toEqual(expect.objectContaining({ youtube: "failed-first-chain-token" }));

    emit(retryArgs.requestId, "youtube", "done");
    retryYoutube.resolve(page(
      [item("Recovered YouTube", "youtube")],
      { youtube: "next-token", soundcloud: 20 },
    ));
    await retry;

    const section = useSearchPageStore.getState().sections[ItemType.Track];
    expect(section.items.map((entry) => entry.name).sort()).toEqual([
      "First page result",
      "Recovered YouTube",
    ]);
    expect(section.nextCursor).toEqual({ youtube: "next-token", soundcloud: 20 });
  });
});
