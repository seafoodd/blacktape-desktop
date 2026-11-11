import { albumYearLabel, albumGenres, albumMetaLine } from "./albumMeta.ts";
import { describe, it, expect } from "vitest";

describe("albumYearLabel", () => {
  it("returns null when there are no songs", () => {
    expect(albumYearLabel([])).toBeNull();
  });

  it("returns null when every year is null, undefined, zero or negative", () => {
    const songs = [
      { release_year: null },
      { release_year: undefined },
      { release_year: 0 },
      { release_year: -5 },
    ];
    expect(albumYearLabel(songs)).toBeNull();
  });

  it("returns the single year for one song", () => {
    expect(albumYearLabel([{ release_year: 2014 }])).toBe("2014");
  });

  it("returns the year once when several songs share it", () => {
    const songs = [
      { release_year: 2014 },
      { release_year: 2014 },
      { release_year: 2014 },
    ];
    expect(albumYearLabel(songs)).toBe("2014");
  });

  it("returns an en dash range when the songs have different years", () => {
    const songs = [
      { release_year: 2014 },
      { release_year: 2012 },
      { release_year: 2013 },
    ];
    expect(albumYearLabel(songs)).toBe("2012–2014");
  });

  it("ignores missing years when computing the range from valid ones", () => {
    const songs = [
      { release_year: null },
      { release_year: 2012 },
      { release_year: undefined },
      { release_year: 2014 },
      { release_year: 0 },
    ];
    expect(albumYearLabel(songs)).toBe("2012–2014");
  });
});

describe("albumGenres", () => {
  it("returns an empty array when there are no songs", () => {
    expect(albumGenres([])).toEqual([]);
  });

  it("returns an empty array for null, empty or whitespace-only genres", () => {
    const songs = [
      { genre: null },
      { genre: "" },
      { genre: "   " },
      { genre: undefined },
    ];
    expect(albumGenres(songs)).toEqual([]);
  });

  it("splits genres on semicolon, slash and comma", () => {
    expect(albumGenres([{ genre: "Electronic;IDM/Ambient,Techno" }])).toEqual([
      "Electronic",
      "IDM",
      "Ambient",
      "Techno",
    ]);
  });

  it("trims whitespace around each genre", () => {
    expect(albumGenres([{ genre: "  Electronic ;  IDM  " }])).toEqual([
      "Electronic",
      "IDM",
    ]);
  });

  it("dedupes case-insensitively, keeping the first spelling and order of appearance", () => {
    const songs = [
      { genre: "Electronic; IDM" },
      { genre: "idm" },
      { genre: "Ambient" },
    ];
    expect(albumGenres(songs)).toEqual(["Electronic", "IDM", "Ambient"]);
  });

  it("dedupes repeated genres within a single tag", () => {
    expect(albumGenres([{ genre: "Rock, rock, ROCK" }])).toEqual(["Rock"]);
  });
});

describe("albumMetaLine", () => {
  const songs = [
    { release_year: 2014, genre: "Electronic; IDM" },
    { release_year: 2014, genre: "IDM" },
  ];

  it("joins year and genres with a middle dot when both are present", () => {
    expect(albumMetaLine(songs)).toBe("2014 · Electronic, IDM");
  });

  it("returns only the year when there are no genres", () => {
    expect(albumMetaLine([{ release_year: 2014, genre: null }])).toBe("2014");
  });

  it("returns only the genres when there is no year", () => {
    expect(
      albumMetaLine([{ release_year: null, genre: "Electronic; IDM" }]),
    ).toBe("Electronic, IDM");
  });

  it("returns null when there is neither a year nor genres", () => {
    expect(albumMetaLine([])).toBeNull();
    expect(albumMetaLine([{ release_year: null, genre: "  " }])).toBeNull();
  });

  it("truncates genres to 4 by default", () => {
    const six = [{ release_year: 2014, genre: "A, B, C, D, E, F" }];
    expect(albumMetaLine(six)).toBe("2014 · A, B, C, D");
  });

  it("truncates genres to a custom maxGenres", () => {
    const six = [{ release_year: 2014, genre: "A, B, C, D, E, F" }];
    expect(albumMetaLine(six, 1)).toBe("2014 · A");
  });
});
