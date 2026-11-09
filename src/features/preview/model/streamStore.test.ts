import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  ERROR_VISIBLE_MS,
  isStreamingUrl,
  useStreamStore,
} from "./streamStore.ts";

const { streamPlayMock, streamPrefetchMock, audioState } = vi.hoisted(() => ({
  streamPlayMock: vi.fn(),
  streamPrefetchMock: vi.fn(),
  audioState: {
    currentSong: null as { source_url?: string | null } | null,
    isStreaming: false,
    togglePlay: vi.fn(),
  },
}));

vi.mock("@/shared/lib/stream.ts", () => ({
  streamPlay: streamPlayMock,
  streamPrefetch: streamPrefetchMock,
}));
vi.mock("@/shared/store/audioStore.ts", () => ({
  useAudioStore: { getState: () => audioState },
}));

let counter = 0;
const item = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => {
  counter += 1;
  return {
    item_type: ItemType.Track,
    name: `Track ${counter}`,
    band_name: "Band",
    item_url_path: `https://example.test/track/${counter}`,
    img: "https://example.test/cover.jpg",
    platform: "youtube",
    duration: 200,
    ...over,
  };
};

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

const store = () => useStreamStore.getState();

beforeEach(() => {
  streamPlayMock.mockReset();
  streamPlayMock.mockResolvedValue(undefined);
  streamPrefetchMock.mockReset();
  streamPrefetchMock.mockResolvedValue(undefined);
  audioState.currentSong = null;
  audioState.isStreaming = false;
  audioState.togglePlay.mockReset();
  audioState.togglePlay.mockResolvedValue(undefined);
  useStreamStore.setState({ loadingUrl: null, error: null });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("isStreamingUrl", () => {
  it("is true only when streaming and the current song is the url", () => {
    const url = "https://example.test/a";
    expect(isStreamingUrl({ source_url: url }, true, url)).toBe(true);
  });

  it("is false without a current song, when not streaming, or for another url", () => {
    const url = "https://example.test/a";
    expect(isStreamingUrl(null, true, url)).toBe(false);
    expect(isStreamingUrl({ source_url: url }, false, url)).toBe(false);
    expect(isStreamingUrl({ source_url: "https://example.test/b" }, true, url)).toBe(false);
    expect(isStreamingUrl({ source_url: null }, true, url)).toBe(false);
  });
});

describe("play", () => {
  it("sets loadingUrl while streamPlay is pending and clears it after it resolves", async () => {
    const pending = deferred<void>();
    streamPlayMock.mockReturnValueOnce(pending.promise);
    const track = item();

    const playing = store().play(track);
    expect(store().loadingUrl).toBe(track.item_url_path);
    expect(store().error).toBeNull();

    pending.resolve(undefined);
    await playing;
    expect(store().loadingUrl).toBeNull();
    expect(store().error).toBeNull();
  });

  it("calls streamPlay with the item's metadata in milliseconds", async () => {
    const track = item({ name: "Song", band_name: "Artist", img: "cover.png", duration: 200 });

    await store().play(track);

    expect(streamPlayMock).toHaveBeenCalledTimes(1);
    expect(streamPlayMock).toHaveBeenCalledWith({
      platform: "youtube",
      url: track.item_url_path,
      title: "Song",
      artist: "Artist",
      cover: "cover.png",
      duration_ms: 200000,
    });
  });

  it("sends cover null for an empty image", async () => {
    await store().play(item({ img: "" }));
    expect(streamPlayMock.mock.calls[0][0].cover).toBeNull();
  });

  it("sends duration_ms null when the duration is missing", async () => {
    await store().play(item({ duration: undefined as unknown as number }));
    expect(streamPlayMock.mock.calls[0][0].duration_ms).toBeNull();
  });

  it("toggles pause instead of streaming when the item is the one streaming", async () => {
    const track = item();
    audioState.currentSong = { source_url: track.item_url_path };
    audioState.isStreaming = true;

    await store().play(track);

    expect(audioState.togglePlay).toHaveBeenCalledTimes(1);
    expect(streamPlayMock).not.toHaveBeenCalled();
    expect(store().loadingUrl).toBeNull();
  });

  it("stores a string rejection as the error, clears loadingUrl and hides the error after ERROR_VISIBLE_MS", async () => {
    vi.useFakeTimers();
    streamPlayMock.mockRejectedValueOnce("unavailable");
    const track = item();

    await store().play(track);

    expect(store().error).toEqual({ url: track.item_url_path, message: "unavailable" });
    expect(store().loadingUrl).toBeNull();

    vi.advanceTimersByTime(ERROR_VISIBLE_MS - 1);
    expect(store().error).not.toBeNull();

    vi.advanceTimersByTime(1);
    expect(store().error).toBeNull();
  });

  it("a newer error for another url is not cleared by the older error's timer", async () => {
    vi.useFakeTimers();
    const first = item();
    const second = item();

    streamPlayMock.mockRejectedValueOnce("first failed");
    await store().play(first);
    expect(store().error?.url).toBe(first.item_url_path);

    vi.advanceTimersByTime(3000);

    streamPlayMock.mockRejectedValueOnce("second failed");
    await store().play(second);
    expect(store().error).toEqual({ url: second.item_url_path, message: "second failed" });

    // Past the first error's deadline: the newer error must still be visible.
    vi.advanceTimersByTime(2500);
    expect(store().error).toEqual({ url: second.item_url_path, message: "second failed" });

    // Past the newer error's own deadline: now it is cleared.
    vi.advanceTimersByTime(ERROR_VISIBLE_MS);
    expect(store().error).toBeNull();
  });

  it("keeps loadingUrl on the newer play when the older one settles first", async () => {
    const older = deferred<void>();
    const newer = deferred<void>();
    streamPlayMock.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    const first = item();
    const second = item();

    const firstPlay = store().play(first);
    const secondPlay = store().play(second);
    expect(store().loadingUrl).toBe(second.item_url_path);

    older.resolve(undefined);
    await firstPlay;
    expect(store().loadingUrl).toBe(second.item_url_path);

    newer.resolve(undefined);
    await secondPlay;
    expect(store().loadingUrl).toBeNull();
  });
});

describe("prefetch", () => {
  it("warms a url once per platform and url", () => {
    const track = item();

    store().prefetch(track);
    store().prefetch(track);

    expect(streamPrefetchMock).toHaveBeenCalledTimes(1);
    expect(streamPrefetchMock).toHaveBeenCalledWith("youtube", track.item_url_path);
  });

  it("warms the same url again for another platform", () => {
    const track = item();

    store().prefetch(track);
    store().prefetch({ ...track, platform: "bandcamp" });

    expect(streamPrefetchMock).toHaveBeenCalledTimes(2);
    expect(streamPrefetchMock).toHaveBeenLastCalledWith("bandcamp", track.item_url_path);
  });
});
