import { describe, expect, it } from "vitest";
import { albumHasMultipleDiscs, playableRowsFrom, rowDiscNumber } from "./artistAlbumsDisc.ts";
import type { Song } from "@/shared/types.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";

const song = (disc_number?: number | null): Song =>
  ({
    id: 1,
    title: "Track",
    disc_number,
  }) as Song;

const missing = (disc_number?: number | null): AlbumTrackInfo =>
  ({
    index: 0,
    title: "Missing",
    disc_number,
  }) as AlbumTrackInfo;

describe("artistAlbumsDisc helper", () => {
  it("detects single disc album when disc_number is missing or 1", () => {
    expect(albumHasMultipleDiscs([song(undefined), song(null), song(1)])).toBe(false);
  });

  it("detects multi-disc album when any song has disc_number > 1", () => {
    expect(albumHasMultipleDiscs([song(1), song(2)])).toBe(true);
  });

  it("detects multi-disc album when missing track has disc_number > 1", () => {
    expect(albumHasMultipleDiscs([song(1)], [missing(2)])).toBe(true);
  });

  it("extracts disc number falling back to 1", () => {
    expect(rowDiscNumber({ kind: "song", song: song(2) })).toBe(2);
    expect(rowDiscNumber({ kind: "song", song: song(null) })).toBe(1);
    expect(rowDiscNumber({ kind: "missing", track: missing(3) })).toBe(3);
    expect(rowDiscNumber({ kind: "missing", track: missing(undefined) })).toBe(1);
  });
});
it("starts Breed at its playable queue position when an earlier album has a placeholder", () => {
  const queue = playableRowsFrom([1, -1, 4, -2], 4, (id) => id === -1 ? undefined : id);
  expect(queue).toEqual({ entries: [1, 4, -2], index: 1 });
});
