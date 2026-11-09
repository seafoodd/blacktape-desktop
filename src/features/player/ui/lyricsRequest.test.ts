import { describe, it, expect, vi } from "vitest";
import type { Song } from "@/shared/types.ts";
import { lyricsRequest } from "./Lyrics.tsx";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const librarySong = (over: Partial<Song> = {}): Song => ({
  id: 5,
  path: "/music/a.mp3",
  title: "T",
  artists: ["A"],
  album_artist: "AA",
  album: "Al",
  duration_ms: 200_000,
  source: "local",
  canonical_track_slug: "t",
  canonical_album_slug: "al",
  quality_tier: "standard",
  ...over,
});

/** A streamed song: no library id. */
const streamSong = (over: Partial<Song> = {}): Song =>
  ({ ...librarySong(), id: null, ...over }) as unknown as Song;

describe("lyricsRequest for library songs", () => {
  it("loads the stored lyrics by id", () => {
    expect(lyricsRequest(librarySong())).toEqual(["get_lyrics", { id: 5 }]);
  });

  it("refetches by id from the given provider", () => {
    expect(lyricsRequest(librarySong(), "lrclib", true)).toEqual([
      "refetch_lyrics",
      { id: 5, provider: "lrclib" },
    ]);
  });

  it("refetch without a provider uses a null provider", () => {
    expect(lyricsRequest(librarySong(), undefined, true)).toEqual([
      "refetch_lyrics",
      { id: 5, provider: null },
    ]);
  });

  it("ignores the provider when not refetching", () => {
    expect(lyricsRequest(librarySong(), "genius")).toEqual(["get_lyrics", { id: 5 }]);
  });
});

describe("lyricsRequest for streamed songs", () => {
  it("looks the lyrics up by artist and title", () => {
    expect(
      lyricsRequest(streamSong({ artists: ["A"], title: "T", album: "", duration_ms: 0 })),
    ).toEqual([
      "get_stream_lyrics",
      { artist: "A", title: "T", album: null, durationMs: null, provider: null },
    ]);
  });

  it("falls back to the album artist when there are no artists", () => {
    const [command, args] = lyricsRequest(streamSong({ artists: [], album_artist: "AA" }));
    expect(command).toBe("get_stream_lyrics");
    expect(args.artist).toBe("AA");
  });

  it("passes the album, duration and provider through", () => {
    expect(
      lyricsRequest(
        streamSong({ artists: ["A"], title: "T", album: "Al", duration_ms: 180_000 }),
        "genius",
      ),
    ).toEqual([
      "get_stream_lyrics",
      { artist: "A", title: "T", album: "Al", durationMs: 180_000, provider: "genius" },
    ]);
  });
});
