import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  rememberStreamAlbums,
  startMixedPlayback,
  streamAddToQueue,
  streamAlbumUrl,
  streamQueue,
  type StreamRequest,
} from "./stream.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

// The album map is module-level, so every test uses urls no other test uses.
let counter = 0;
const request = (over: Partial<StreamRequest> = {}): StreamRequest => {
  counter += 1;
  return {
    platform: "youtube",
    url: `https://yt.test/stream-${counter}`,
    title: `Song ${counter}`,
    artist: "Band",
    ...over,
  };
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
});

describe("rememberStreamAlbums / streamAlbumUrl", () => {
  it("remembers the album url of a request by its track url", () => {
    const r = request({ album_url: "https://alb/remembered" });
    rememberStreamAlbums([r]);
    expect(streamAlbumUrl(r.url)).toBe("https://alb/remembered");
  });

  it("does not remember a request without an album url", () => {
    const r = request();
    rememberStreamAlbums([r]);
    expect(streamAlbumUrl(r.url)).toBeUndefined();
  });

  it("does not remember a request with a null album url", () => {
    const r = request({ album_url: null });
    rememberStreamAlbums([r]);
    expect(streamAlbumUrl(r.url)).toBeUndefined();
  });

  it("remembers each request of a list", () => {
    const a = request({ album_url: "https://alb/a" });
    const b = request({ album_url: "https://alb/b" });
    const c = request();
    rememberStreamAlbums([a, b, c]);
    expect(streamAlbumUrl(a.url)).toBe("https://alb/a");
    expect(streamAlbumUrl(b.url)).toBe("https://alb/b");
    expect(streamAlbumUrl(c.url)).toBeUndefined();
  });

  it("returns undefined for a missing, null or empty url", () => {
    expect(streamAlbumUrl(undefined)).toBeUndefined();
    expect(streamAlbumUrl(null)).toBeUndefined();
    expect(streamAlbumUrl("")).toBeUndefined();
  });

  it("returns undefined for a url never remembered", () => {
    expect(streamAlbumUrl("https://yt.test/never-seen")).toBeUndefined();
  });
});

describe("streamQueue", () => {
  it("remembers the album urls and invokes stream_queue with the same args", async () => {
    const a = request({ album_url: "https://alb/queue" });
    const b = request();

    await streamQueue([a, b], 1, true);

    expect(streamAlbumUrl(a.url)).toBe("https://alb/queue");
    expect(streamAlbumUrl(b.url)).toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("stream_queue", { requests: [a, b], index: 1, shuffle: true });
  });

  it("defaults shuffle to false", async () => {
    const a = request();
    await streamQueue([a], 0);
    expect(invokeMock).toHaveBeenCalledWith("stream_queue", { requests: [a], index: 0, shuffle: false });
  });
});

describe("streamAddToQueue", () => {
  it("remembers the album urls and invokes stream_add_to_queue with the same args", async () => {
    const a = request({ album_url: "https://alb/add" });

    await streamAddToQueue([a], true);

    expect(streamAlbumUrl(a.url)).toBe("https://alb/add");
    expect(invokeMock).toHaveBeenCalledWith("stream_add_to_queue", { requests: [a], playNext: true });
  });

  it("defaults playNext to false", async () => {
    const a = request();
    await streamAddToQueue([a]);
    expect(invokeMock).toHaveBeenCalledWith("stream_add_to_queue", { requests: [a], playNext: false });
  });
});

describe("startMixedPlayback", () => {
  it("remembers only the album urls of stream entries and invokes start_mixed_playback", async () => {
    const stream = request({ album_url: "https://alb/mixed" });
    const streamNoAlbum = request();
    const entries = [
      { kind: "song" as const, id: 7 },
      { kind: "stream" as const, ...stream },
      { kind: "stream" as const, ...streamNoAlbum },
    ];

    await startMixedPlayback(entries, 1);

    expect(streamAlbumUrl(stream.url)).toBe("https://alb/mixed");
    expect(streamAlbumUrl(streamNoAlbum.url)).toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("start_mixed_playback", { entries, index: 1 });
  });

  it("ignores library song entries", async () => {
    const song = { kind: "song" as const, id: 42 };
    await startMixedPlayback([song], 0);
    expect(streamAlbumUrl("42")).toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("start_mixed_playback", { entries: [song], index: 0 });
  });
});
