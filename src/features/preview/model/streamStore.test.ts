import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  ERROR_VISIBLE_MS,
  isStreamingUrl,
  queueFrom,
  toStreamRequest,
  useStreamStore,
} from "./streamStore.ts";

const { streamQueueMock, streamPrefetchMock, audioState } = vi.hoisted(() => ({
  streamQueueMock: vi.fn(),
  streamPrefetchMock: vi.fn(),
  audioState: {
    currentSong: null as { source_url?: string | null } | null,
    isStreaming: false,
    togglePlay: vi.fn(),
  },
}));

vi.mock("@/shared/lib/stream.ts", () => ({
  streamQueue: streamQueueMock,
  streamPrefetch: streamPrefetchMock,
}));
vi.mock("@/shared/store/audioStore.ts", () => ({
  useAudioStore: { getState: () => audioState },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@/shared/lib/search.ts", () => ({ searchPlatforms: vi.fn() }));

let counter = 0;
const fixture = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => {
  counter += 1;
  return {
    item_type: ItemType.Track,
    name: `Song ${counter}`,
    band_name: "Band",
    item_url_path: `https://yt.test/watch?v=${counter}`,
    img: "https://img.test/cover.jpg",
    platform: "youtube",
    duration: 200,
    ...over,
  };
};

const store = () => useStreamStore.getState();

beforeEach(() => {
  streamQueueMock.mockReset();
  streamPrefetchMock.mockReset();
  streamPrefetchMock.mockResolvedValue(undefined);
  audioState.currentSong = null;
  audioState.isStreaming = false;
  audioState.togglePlay.mockReset();
  useStreamStore.setState({ pendingUrl: null, error: null });
});

describe("isStreamingUrl", () => {
  it("is false when nothing is playing", () => {
    expect(isStreamingUrl(null, false, "u")).toBe(false);
  });

  it("is false when the song is the same url but not a stream", () => {
    expect(isStreamingUrl({ source_url: "u" }, false, "u")).toBe(false);
  });

  it("is false when a stream of another url is playing", () => {
    expect(isStreamingUrl({ source_url: "other" }, true, "u")).toBe(false);
  });

  it("is false when the current song has no source url", () => {
    expect(isStreamingUrl({ source_url: null }, true, "u")).toBe(false);
    expect(isStreamingUrl({}, true, "u")).toBe(false);
  });

  it("is true only when streaming and the source url matches", () => {
    expect(isStreamingUrl({ source_url: "u" }, true, "u")).toBe(true);
  });
});

describe("toStreamRequest", () => {
  it("maps the item fields to a stream request", () => {
    const item = fixture({
      item_url_path: "https://yt.test/x",
      name: "Title",
      band_name: "Artist",
      img: "https://img.test/c.jpg",
      platform: "soundcloud",
      duration: 200,
    });
    expect(toStreamRequest(item)).toEqual({
      platform: "soundcloud",
      url: "https://yt.test/x",
      title: "Title",
      artist: "Artist",
      cover: "https://img.test/c.jpg",
      duration_ms: 200_000,
    });
  });

  it("maps an empty image to a null cover", () => {
    expect(toStreamRequest(fixture({ img: "" })).cover).toBeNull();
  });

  it("maps a missing duration to a null duration_ms", () => {
    expect(toStreamRequest(fixture({ duration: undefined })).duration_ms).toBeNull();
  });
});

describe("queueFrom", () => {
  it("starts with the item followed by the items after it in the list", () => {
    const [a, b, c, d] = [fixture(), fixture(), fixture(), fixture()];
    expect(queueFrom(b, [a, b, c, d])).toEqual([b, c, d]);
  });

  it("skips duplicates by url, including the item itself appearing again later", () => {
    const a = fixture();
    const b = fixture();
    const c = fixture();
    const cDup = fixture({ item_url_path: c.item_url_path, name: "dup" });
    const aDup = fixture({ item_url_path: a.item_url_path, name: "dup a" });
    expect(queueFrom(a, [a, b, c, cDup, aDup])).toEqual([a, b, c]);
  });

  it("returns just the item when it is not in the list", () => {
    const x = fixture();
    expect(queueFrom(x, [fixture(), fixture()])).toEqual([x]);
  });

  it("returns just the item when there is no list", () => {
    const x = fixture();
    expect(queueFrom(x)).toEqual([x]);
  });
});

describe("play", () => {
  it("streams the queue from the item with index 0", async () => {
    streamQueueMock.mockResolvedValue(undefined);
    const [a, b, c] = [fixture(), fixture(), fixture()];

    await store().play(b, [a, b, c]);

    expect(streamQueueMock).toHaveBeenCalledTimes(1);
    const [requests, index] = streamQueueMock.mock.calls[0];
    expect(index).toBe(0);
    expect(requests).toEqual([toStreamRequest(b), toStreamRequest(c)]);
  });

  it("sets pendingUrl while the queue is being started, then clears it", async () => {
    let resolveQueue!: () => void;
    streamQueueMock.mockReturnValue(
      new Promise<void>((resolve) => {
        resolveQueue = resolve;
      }),
    );
    const item = fixture();

    const pending = store().play(item);
    expect(store().pendingUrl).toBe(item.item_url_path);

    resolveQueue();
    await pending;
    expect(store().pendingUrl).toBeNull();
    expect(store().error).toBeNull();
  });

  it("toggles pause instead of streaming when the url is the one playing", async () => {
    const item = fixture();
    audioState.currentSong = { source_url: item.item_url_path };
    audioState.isStreaming = true;

    await store().play(item, [item]);

    expect(audioState.togglePlay).toHaveBeenCalledTimes(1);
    expect(streamQueueMock).not.toHaveBeenCalled();
    expect(store().pendingUrl).toBeNull();
  });
});

describe("errors", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("records the rejection of the queue as an error of the url", async () => {
    streamQueueMock.mockRejectedValue("boom");
    const item = fixture();

    await store().play(item);

    expect(store().error).toEqual({ url: item.item_url_path, message: "boom" });
    expect(store().pendingUrl).toBeNull();
  });

  it("stringifies a non-string rejection", async () => {
    streamQueueMock.mockRejectedValue(new Error("nope"));
    const item = fixture();

    await store().play(item);

    expect(store().error).toEqual({ url: item.item_url_path, message: "Error: nope" });
  });

  it("clears the error after ERROR_VISIBLE_MS", async () => {
    streamQueueMock.mockRejectedValue("boom");
    const item = fixture();
    await store().play(item);

    vi.advanceTimersByTime(ERROR_VISIBLE_MS - 1);
    expect(store().error).not.toBeNull();

    vi.advanceTimersByTime(1);
    expect(store().error).toBeNull();
  });

  it("a newer error replaces the older one and the older timer does not clear it", () => {
    store().fail({ url: "a", message: "first" });
    vi.advanceTimersByTime(3000);

    store().fail({ url: "b", message: "second" });
    // t = 6000: the first error's timer would have fired at 5000.
    vi.advanceTimersByTime(3000);
    expect(store().error).toEqual({ url: "b", message: "second" });

    // t = 8000: the second error's timer fires at 8000.
    vi.advanceTimersByTime(2000);
    expect(store().error).toBeNull();
  });
});

describe("prefetch", () => {
  it("warms the url once per platform and url", () => {
    const item = fixture({ platform: "youtube" });

    store().prefetch(item);
    store().prefetch(item);

    expect(streamPrefetchMock).toHaveBeenCalledTimes(1);
    expect(streamPrefetchMock).toHaveBeenCalledWith("youtube", item.item_url_path);
  });

  it("warms the same url again on another platform", () => {
    const item = fixture({ platform: "youtube" });
    store().prefetch(item);
    store().prefetch({ ...item, platform: "bandcamp" });

    expect(streamPrefetchMock).toHaveBeenCalledTimes(2);
    expect(streamPrefetchMock).toHaveBeenLastCalledWith("bandcamp", item.item_url_path);
  });

  it("warms a different url on the same platform", () => {
    store().prefetch(fixture());
    store().prefetch(fixture());

    expect(streamPrefetchMock).toHaveBeenCalledTimes(2);
  });
});
