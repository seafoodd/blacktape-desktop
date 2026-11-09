import { describe, it, expect } from "vitest";
import { SONG_DRAG_MIME, encodeSongDrag, decodeSongDrag } from "./dragSongs.ts";

describe("SONG_DRAG_MIME", () => {
  it("is application/json", () => {
    expect(SONG_DRAG_MIME).toBe("application/json");
  });
});

describe("encodeSongDrag / decodeSongDrag round trip", () => {
  it("round trips a single id", () => {
    expect(decodeSongDrag(encodeSongDrag([42]))).toEqual([42]);
  });

  it("round trips several ids and preserves order", () => {
    const ids = [7, 2, 9, 1];
    expect(decodeSongDrag(encodeSongDrag(ids))).toEqual([7, 2, 9, 1]);
  });

  it("encodes an empty list that decodes to []", () => {
    expect(decodeSongDrag(encodeSongDrag([]))).toEqual([]);
  });
});

describe("encodeSongDrag legacy songId", () => {
  it("sets songId to the first id", () => {
    const parsed = JSON.parse(encodeSongDrag([10, 20, 30]));
    expect(parsed.songId).toBe(10);
    expect(parsed.songIds).toEqual([10, 20, 30]);
  });
});

describe("decodeSongDrag legacy and malformed payloads", () => {
  it("decodes the old single payload {songId}", () => {
    expect(decodeSongDrag('{"songId":5}')).toEqual([5]);
  });

  it("filters non-number entries out of songIds", () => {
    expect(decodeSongDrag('{"songIds":[1,"x",3]}')).toEqual([1, 3]);
  });

  it("prefers songIds over songId when both are present", () => {
    expect(decodeSongDrag('{"songIds":[1,2],"songId":9}')).toEqual([1, 2]);
  });

  it("returns [] for an empty string", () => {
    expect(decodeSongDrag("")).toEqual([]);
  });

  it("returns [] for invalid JSON", () => {
    expect(decodeSongDrag("{not json")).toEqual([]);
  });

  it('returns [] for "null"', () => {
    expect(decodeSongDrag("null")).toEqual([]);
  });

  it('returns [] for "{}"', () => {
    expect(decodeSongDrag("{}")).toEqual([]);
  });

  it("returns [] when songIds is not an array", () => {
    expect(decodeSongDrag('{"songIds":"nope"}')).toEqual([]);
  });
});
