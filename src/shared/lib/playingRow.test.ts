import { findPlayingRow } from "./playingRow.ts";
import { describe, it, expect } from "vitest";

const toSongs = (ids: number[]) => ids.map((id) => ({ id }));

describe("findPlayingRow", () => {
  it("returns null when currentId is null or undefined", () => {
    const songs = toSongs([1, 2, 3]);
    expect(findPlayingRow(songs, null, 0)).toBeNull();
    expect(findPlayingRow(songs, undefined, 0)).toBeNull();
    expect(findPlayingRow(songs, null, null)).toBeNull();
  });

  it("returns the preferred index when it holds the playing song, even if an earlier duplicate exists", () => {
    const songs = toSongs([1, 2, 1, 3]);
    expect(findPlayingRow(songs, 1, 2)).toBe(2);
  });

  it("returns index 0 when the preferred index is 0 and holds the playing song", () => {
    const songs = toSongs([1, 2, 1, 3]);
    expect(findPlayingRow(songs, 1, 0)).toBe(0);
  });

  it("falls back to the first match when the preferred index points at a different song", () => {
    const songs = toSongs([1, 2, 1, 3]);
    expect(findPlayingRow(songs, 1, 1)).toBe(0);
  });

  it("returns the first match when preferredIndex is null or undefined", () => {
    const songs = toSongs([1, 2, 1, 3]);
    expect(findPlayingRow(songs, 1, null)).toBe(0);
    expect(findPlayingRow(songs, 1, undefined)).toBe(0);
  });

  it("returns the first match when preferredIndex is out of range", () => {
    const songs = toSongs([1, 2, 1, 3]);
    expect(findPlayingRow(songs, 1, 10)).toBe(0);
  });

  it("returns null when the playing song is not in the list", () => {
    const songs = toSongs([1, 2, 3]);
    expect(findPlayingRow(songs, 99, 0)).toBeNull();
    expect(findPlayingRow(songs, 99, null)).toBeNull();
  });

  it("returns null for an empty list", () => {
    expect(findPlayingRow([], 1, 0)).toBeNull();
    expect(findPlayingRow([], 1, null)).toBeNull();
  });
});
