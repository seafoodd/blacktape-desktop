import { describe, expect, it, vi } from "vitest";
import { songChanged } from "./audioStore.ts";

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
