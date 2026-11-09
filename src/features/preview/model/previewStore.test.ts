import { describe, it, expect, vi, beforeEach } from "vitest";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { IDLE_PREVIEW, type PreviewState } from "@/shared/lib/preview.ts";
import { isPreviewing, usePreviewStore } from "./previewStore.ts";

const mocks = vi.hoisted(() => ({
  previewPlay: vi.fn(),
  previewPrefetch: vi.fn(),
  previewSeek: vi.fn(),
  previewStop: vi.fn(),
  previewToggle: vi.fn(),
}));

vi.mock("@/shared/lib/preview.ts", async () => {
  const actual = await vi.importActual<typeof import("@/shared/lib/preview.ts")>(
    "@/shared/lib/preview.ts",
  );
  return {
    ...actual,
    previewPlay: mocks.previewPlay,
    previewPrefetch: mocks.previewPrefetch,
    previewSeek: mocks.previewSeek,
    previewStop: mocks.previewStop,
    previewToggle: mocks.previewToggle,
  };
});

let counter = 0;
const item = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => {
  counter += 1;
  return {
    item_type: ItemType.Track,
    name: `Track ${counter}`,
    band_name: "Band",
    item_url_path: `https://example.test/preview/${counter}`,
    img: "https://img.test/cover.jpg",
    platform: "youtube",
    duration: 200,
    ...over,
  };
};

const withState = (over: Partial<PreviewState>): PreviewState => ({
  ...IDLE_PREVIEW,
  ...over,
});

const store = () => usePreviewStore.getState();

beforeEach(() => {
  mocks.previewPlay.mockReset().mockResolvedValue(undefined);
  mocks.previewPrefetch.mockReset().mockResolvedValue(undefined);
  mocks.previewSeek.mockReset().mockResolvedValue(undefined);
  mocks.previewStop.mockReset().mockResolvedValue(undefined);
  mocks.previewToggle.mockReset().mockResolvedValue(undefined);
  usePreviewStore.setState({ state: IDLE_PREVIEW });
});

describe("isPreviewing", () => {
  it("is true for loading, playing and paused of the same url", () => {
    for (const status of ["loading", "playing", "paused"] as const) {
      expect(isPreviewing(withState({ status, url: "u1" }), "u1")).toBe(true);
    }
  });

  it("is false for idle and error of the same url", () => {
    expect(isPreviewing(withState({ status: "idle", url: "u1" }), "u1")).toBe(false);
    expect(isPreviewing(withState({ status: "error", url: "u1" }), "u1")).toBe(false);
  });

  it("is false for another url", () => {
    expect(isPreviewing(withState({ status: "playing", url: "u2" }), "u1")).toBe(false);
    expect(isPreviewing(IDLE_PREVIEW, "u1")).toBe(false);
  });
});

describe("play", () => {
  it("sets loading optimistically with the item metadata and calls previewPlay", async () => {
    const track = item({ img: "https://img.test/a.jpg", duration: 180 });
    const pending = store().play(track);

    expect(store().state).toEqual({
      ...IDLE_PREVIEW,
      status: "loading",
      platform: "youtube",
      url: track.item_url_path,
      title: track.name,
      artist: track.band_name,
      cover: "https://img.test/a.jpg",
      duration_ms: 180_000,
    });
    expect(mocks.previewPlay).toHaveBeenCalledWith({
      platform: "youtube",
      url: track.item_url_path,
      title: track.name,
      artist: track.band_name,
      cover: "https://img.test/a.jpg",
      duration_ms: 180_000,
    });
    await pending;
  });

  it("uses a null cover when the item has no image", async () => {
    const track = item({ img: "" });
    await store().play(track);

    expect(store().state.cover).toBeNull();
    expect(mocks.previewPlay).toHaveBeenCalledWith(expect.objectContaining({ cover: null }));
  });

  it("calls previewToggle and not previewPlay when the same url is playing", async () => {
    const track = item();
    usePreviewStore.setState({
      state: withState({ status: "playing", url: track.item_url_path, platform: "youtube" }),
    });

    await store().play(track);

    expect(mocks.previewToggle).toHaveBeenCalledTimes(1);
    expect(mocks.previewPlay).not.toHaveBeenCalled();
  });

  it("calls previewPlay again when the same url is still loading", async () => {
    const track = item();
    usePreviewStore.setState({
      state: withState({ status: "loading", url: track.item_url_path, platform: "youtube" }),
    });

    await store().play(track);

    expect(mocks.previewToggle).not.toHaveBeenCalled();
    expect(mocks.previewPlay).toHaveBeenCalledTimes(1);
  });

  it("switches to error with the message when previewPlay rejects while still loading", async () => {
    const track = item();
    mocks.previewPlay.mockRejectedValueOnce("stream unavailable");

    await store().play(track);

    expect(store().state.status).toBe("error");
    expect(store().state.error).toBe("stream unavailable");
    expect(store().state.url).toBe(track.item_url_path);
  });

  it("keeps the backend state when previewPlay rejects after the backend already moved on", async () => {
    const track = item();
    const other = "https://example.test/other";
    mocks.previewPlay.mockImplementationOnce(async () => {
      usePreviewStore.setState({ state: withState({ status: "playing", url: other }) });
      throw "late failure";
    });

    await store().play(track);

    expect(store().state.status).toBe("playing");
    expect(store().state.url).toBe(other);
    expect(store().state.error).toBeNull();
  });
});

describe("seek", () => {
  it("moves the position right away and calls previewSeek", async () => {
    usePreviewStore.setState({
      state: withState({ status: "playing", url: "u1", duration_ms: 200_000 }),
    });

    const pending = store().seek(0.5);
    expect(store().state.position_ms).toBe(100_000);
    expect(mocks.previewSeek).toHaveBeenCalledWith(0.5);
    await pending;
  });

  it("only calls previewSeek when the duration is unknown", async () => {
    usePreviewStore.setState({
      state: withState({ status: "playing", url: "u1", duration_ms: null, position_ms: 42 }),
    });

    await store().seek(0.5);

    expect(store().state.position_ms).toBe(42);
    expect(mocks.previewSeek).toHaveBeenCalledWith(0.5);
  });

  it("does not throw when previewSeek rejects", async () => {
    mocks.previewSeek.mockRejectedValueOnce("seek failed");

    await expect(store().seek(0.25)).resolves.toBeUndefined();
    expect(mocks.previewSeek).toHaveBeenCalledWith(0.25);
  });
});

describe("stop", () => {
  it("sets the state to idle and calls previewStop", async () => {
    usePreviewStore.setState({
      state: withState({ status: "playing", url: "u1", title: "x" }),
    });

    await store().stop();

    expect(store().state).toEqual(IDLE_PREVIEW);
    expect(mocks.previewStop).toHaveBeenCalledTimes(1);
  });
});

describe("apply", () => {
  it("replaces the state with the given one", () => {
    const next = withState({ status: "paused", url: "u9", position_ms: 5_000 });

    store().apply(next);

    expect(store().state).toEqual(next);
  });
});

describe("prefetch", () => {
  it("calls previewPrefetch once per platform and url, even when called twice", () => {
    const track = item({ platform: "soundcloud" });

    store().prefetch(track);
    store().prefetch(track);

    expect(mocks.previewPrefetch).toHaveBeenCalledTimes(1);
    expect(mocks.previewPrefetch).toHaveBeenCalledWith("soundcloud", track.item_url_path);
  });

  it("treats the same url on another platform as a separate prefetch", () => {
    const url = "https://example.test/shared";

    store().prefetch(item({ platform: "youtube", item_url_path: url }));
    store().prefetch(item({ platform: "bandcamp", item_url_path: url }));

    expect(mocks.previewPrefetch).toHaveBeenCalledWith("youtube", url);
    expect(mocks.previewPrefetch).toHaveBeenCalledWith("bandcamp", url);
  });
});
