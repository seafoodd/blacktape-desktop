import { describe, expect, it, vi } from "vitest";
import { keepPlaybackError, songChanged } from "./audioStore.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

describe("songChanged", () => {
  it("is false when both sides are null", () => {
    expect(songChanged(null, null)).toBe(false);
  });

  it("is true when a song appears from nothing", () => {
    expect(songChanged(null, { id: 1, source_url: null })).toBe(true);
  });

  it("is true when the song goes away", () => {
    expect(songChanged({ id: 1, source_url: null }, null)).toBe(true);
  });

  it("is false for a stream swapped for its file when the page url matches (different host forms)", () => {
    const stream = { id: null, source_url: "https://www.youtube.com/watch?v=a" };
    const file = { id: 42, source_url: "https://music.youtube.com/watch?v=a&list=x" };
    expect(songChanged(stream, file)).toBe(false);
    expect(songChanged(file, stream)).toBe(false);
  });

  it("is true when the source urls differ", () => {
    expect(
      songChanged(
        { id: 1, source_url: "https://www.youtube.com/watch?v=a" },
        { id: 1, source_url: "https://www.youtube.com/watch?v=b" },
      ),
    ).toBe(true);
  });

  it("falls back to ids when only the previous song has no source_url: same id is not a change", () => {
    expect(songChanged({ id: 7, source_url: null }, { id: 7, source_url: "https://example.com/a" })).toBe(false);
  });

  it("falls back to ids when only the next song has no source_url: different id is a change", () => {
    expect(songChanged({ id: 7, source_url: "https://example.com/a" }, { id: 8, source_url: null })).toBe(true);
  });

  it("falls back to ids when neither side has a source_url", () => {
    expect(songChanged({ id: 3 }, { id: 3 })).toBe(false);
    expect(songChanged({ id: 3 }, { id: 4 })).toBe(true);
  });
});

describe("keepPlaybackError", () => {
  const error = { title: "Playback failed", message: "The stream could not be opened" };

  it("keeps the error when the same song is paused or stopped", () => {
    expect(keepPlaybackError(error, false, false)).toBe(error);
  });

  it("clears the error when another song starts", () => {
    expect(keepPlaybackError(error, true, false)).toBeNull();
    expect(keepPlaybackError(error, true, true)).toBeNull();
  });

  it("clears the error when playback resumes on the same song", () => {
    expect(keepPlaybackError(error, false, true)).toBeNull();
  });

  it("returns the same error object when kept", () => {
    expect(keepPlaybackError(error, false, false)).toBe(error);
  });

  it("keeps null as null", () => {
    expect(keepPlaybackError(null, false, false)).toBeNull();
    expect(keepPlaybackError(null, true, false)).toBeNull();
    expect(keepPlaybackError(null, false, true)).toBeNull();
  });
});
