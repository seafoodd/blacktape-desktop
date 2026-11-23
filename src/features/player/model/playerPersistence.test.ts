import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  parseSavedVolume,
  parseSavedPlayerState,
  parseSavedNavEntry,
  loadSavedVolume,
  saveVolume,
  loadSavedPlayerState,
  savePlayerState,
  loadSavedNavEntry,
  saveNavEntry,
  STORAGE_KEY_VOLUME,
  STORAGE_KEY_PLAYER_STATE,
  STORAGE_KEY_NAV_STATE,
} from "./playerPersistence.ts";
import type { Song } from "@/shared/types.ts";

const mockSong: Song = {
  id: 10,
  path: "/music/song.mp3",
  title: "Test Song",
  artists: ["Artist A"],
  album_artist: "Artist A",
  album: "Album A",
  duration_ms: 200000,
  track_number: 1,
  genre: "Rock",
  release_year: 2024,
  cover_url: null,
  external_cover_url: null,
  lyrics: null,
  lyrics_source: null,
  source: "local",
  source_url: null,
  source_item_id: null,
  canonical_track_slug: "artist-a-test-song",
  canonical_album_slug: "artist-a-album-a",
  quality_tier: "standard",
};

describe("playerPersistence", () => {
  let storageMap: Map<string, string>;

  beforeEach(() => {
    storageMap = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storageMap.get(key) ?? null,
      setItem: (key: string, val: string) => storageMap.set(key, val),
      removeItem: (key: string) => storageMap.delete(key),
      clear: () => storageMap.clear(),
    });
  });

  describe("parseSavedVolume", () => {
    it("parses valid floating point numbers between 0 and 1", () => {
      expect(parseSavedVolume("0.5")).toBe(0.5);
      expect(parseSavedVolume("0")).toBe(0);
      expect(parseSavedVolume("1")).toBe(1);
      expect(parseSavedVolume("0.75")).toBe(0.75);
    });

    it("rejects out-of-bounds or non-numeric values", () => {
      expect(parseSavedVolume("-0.1")).toBeNull();
      expect(parseSavedVolume("1.05")).toBeNull();
      expect(parseSavedVolume("100")).toBeNull();
      expect(parseSavedVolume("abc")).toBeNull();
      expect(parseSavedVolume("NaN")).toBeNull();
      expect(parseSavedVolume("Infinity")).toBeNull();
      expect(parseSavedVolume("")).toBeNull();
      expect(parseSavedVolume("   ")).toBeNull();
      expect(parseSavedVolume(null)).toBeNull();
    });
  });

  describe("parseSavedPlayerState", () => {
    it("parses a valid player state with current song and queue", () => {
      const state = {
        currentSong: mockSong,
        queue: [mockSong],
        queueCursor: 0,
        progress: 0.42,
      };
      const result = parseSavedPlayerState(JSON.stringify(state));
      expect(result).not.toBeNull();
      expect(result?.currentSong?.title).toBe("Test Song");
      expect(result?.queue).toHaveLength(1);
      expect(result?.queueCursor).toBe(0);
      expect(result?.progress).toBe(0.42);
    });

    it("handles null or missing fields gracefully", () => {
      const state = {
        currentSong: null,
        queue: [mockSong],
        queueCursor: null,
        progress: -5,
      };
      const result = parseSavedPlayerState(JSON.stringify(state));
      expect(result).not.toBeNull();
      expect(result?.currentSong).toBeNull();
      expect(result?.queueCursor).toBeNull();
      expect(result?.progress).toBe(0);
    });

    it("returns null for empty or corrupted json", () => {
      expect(parseSavedPlayerState(null)).toBeNull();
      expect(parseSavedPlayerState("")).toBeNull();
      expect(parseSavedPlayerState("{not a valid json")).toBeNull();
      expect(parseSavedPlayerState(JSON.stringify({ currentSong: null, queue: [] }))).toBeNull();
    });
  });

  describe("parseSavedNavEntry", () => {
    it("parses valid navigation entries", () => {
      expect(
        parseSavedNavEntry(
          JSON.stringify({ view: "ARTIST_ALBUMS", artistName: "Radiohead" }),
        ),
      ).toEqual({
        view: "ARTIST_ALBUMS",
        artistName: "Radiohead",
        searchQuery: undefined,
        playlistId: undefined,
        previewItem: undefined,
      });

      expect(
        parseSavedNavEntry(
          JSON.stringify({ view: "PLAYLIST", playlistId: 3 }),
        ),
      ).toEqual({
        view: "PLAYLIST",
        artistName: undefined,
        searchQuery: undefined,
        playlistId: 3,
        previewItem: undefined,
      });

      expect(
        parseSavedNavEntry(JSON.stringify({ view: "TRASH" })),
      ).toEqual({
        view: "TRASH",
        artistName: undefined,
        searchQuery: undefined,
        playlistId: undefined,
        previewItem: undefined,
      });
    });

    it("rejects invalid view names or malformed data", () => {
      expect(parseSavedNavEntry(JSON.stringify({ view: "UNKNOWN_VIEW" }))).toBeNull();
      expect(parseSavedNavEntry(null)).toBeNull();
      expect(parseSavedNavEntry("not-json")).toBeNull();
    });
  });

  describe("localStorage read/write helpers", () => {
    it("saves and loads volume", () => {
      saveVolume(0.65);
      expect(localStorage.getItem(STORAGE_KEY_VOLUME)).toBe("0.65");
      expect(loadSavedVolume()).toBe(0.65);
    });

    it("saves and loads player state", () => {
      const state = {
        currentSong: mockSong,
        queue: [mockSong],
        queueCursor: 0,
        progress: 0.15,
      };
      savePlayerState(state);
      expect(localStorage.getItem(STORAGE_KEY_PLAYER_STATE)).toContain("Test Song");
      const loaded = loadSavedPlayerState();
      expect(loaded?.currentSong?.title).toBe("Test Song");
      expect(loaded?.progress).toBe(0.15);
    });

    it("saves and loads navigation entry", () => {
      const entry = {
        view: "ARTIST_ALBUMS" as const,
        artistName: "Daft Punk",
      };
      saveNavEntry(entry);
      expect(localStorage.getItem(STORAGE_KEY_NAV_STATE)).toContain("Daft Punk");
      const loaded = loadSavedNavEntry();
      expect(loaded?.artistName).toBe("Daft Punk");
    });
  });
});
