import { describe, it, expect } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  DEFAULT_FILTERS,
  applyFilters,
  mergePages,
  normalizeText,
  pickProfileArtist,
  popularityOf,
  sortItems,
  type SearchPageFilters,
} from "./searchPageFilters.ts";

let counter = 0;
const make = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => {
  counter += 1;
  return {
    item_type: ItemType.Track,
    name: `Song ${counter}`,
    band_name: "Band",
    item_url_path: `https://example.test/item/${counter}`,
    img: "",
    platform: "youtube",
    ...over,
  };
};

const withFilters = (patch: Partial<SearchPageFilters>): SearchPageFilters => ({
  ...DEFAULT_FILTERS,
  ...patch,
});

const names = (items: SearchSuggestion[]) => items.map((i) => i.name);

describe("normalizeText", () => {
  it("lower-cases and strips diacritics", () => {
    expect(normalizeText("Beyoncé")).toBe("beyonce");
    expect(normalizeText("Sigur Rós")).toBe("sigur ros");
    expect(normalizeText("MOTÖRHEAD")).toBe("motorhead");
  });

  it("drops apostrophes and turns other punctuation into single spaces", () => {
    expect(normalizeText("Don't Stop")).toBe("dont stop");
    expect(normalizeText("AC/DC")).toBe("ac dc");
    expect(normalizeText("  a -- b  ")).toBe("a b");
  });

  it("handles null, undefined, empty and punctuation-only input", () => {
    expect(normalizeText(null)).toBe("");
    expect(normalizeText(undefined)).toBe("");
    expect(normalizeText("")).toBe("");
    expect(normalizeText("!!!")).toBe("");
  });

  it("keeps non-latin letters", () => {
    expect(normalizeText("Кино")).toBe("кино");
  });
});

describe("applyFilters", () => {
  const items = [
    make({ name: "A", platform: "youtube" }),
    make({ name: "B", platform: "bandcamp" }),
    make({ name: "C", platform: "youtube" }),
  ];

  it("keeps everything with the default filters", () => {
    expect(applyFilters(items, DEFAULT_FILTERS)).toEqual(items);
  });

  it("filters by platform and ignores the tab", () => {
    expect(names(applyFilters(items, withFilters({ platform: "youtube" })))).toEqual(["A", "C"]);
    expect(names(applyFilters(items, withFilters({ platform: "bandcamp" })))).toEqual(["B"]);
    expect(applyFilters(items, withFilters({ platform: "All", tab: ItemType.Album }))).toHaveLength(3);
  });

  it("applies year bounds inclusively, with min only and max only", () => {
    const years = [
      make({ name: "1999", year: 1999 }),
      make({ name: "2000", year: 2000 }),
      make({ name: "2010", year: 2010 }),
      make({ name: "2020", year: 2020 }),
    ];
    expect(names(applyFilters(years, withFilters({ year: { min: 2000, max: 2010 } })))).toEqual(["2000", "2010"]);
    expect(names(applyFilters(years, withFilters({ year: { min: 2010 } })))).toEqual(["2010", "2020"]);
    expect(names(applyFilters(years, withFilters({ year: { max: 2000 } })))).toEqual(["1999", "2000"]);
  });

  it("drops items without a year when a year range is set, keeps them when it is empty", () => {
    const list = [make({ name: "dated", year: 2001 }), make({ name: "undated" })];
    expect(names(applyFilters(list, withFilters({ year: { min: 2000 } })))).toEqual(["dated"]);
    expect(names(applyFilters(list, withFilters({ year: {} })))).toEqual(["dated", "undated"]);
  });

  it("treats runtime null years as missing", () => {
    const list = [
      { ...make({ name: "nullYear" }), year: null },
      make({ name: "ok", year: 2005 }),
    ] as unknown as SearchSuggestion[];
    expect(names(applyFilters(list, withFilters({ year: { min: 2000 } })))).toEqual(["ok"]);
    expect(names(applyFilters(list, withFilters({ year: {} })))).toEqual(["nullYear", "ok"]);
  });

  it("ignores NaN bounds (no bound)", () => {
    const list = [make({ name: "x", year: 1990 })];
    expect(applyFilters(list, withFilters({ year: { min: NaN } }))).toHaveLength(1);
  });

  it("applies duration bounds in seconds and drops items without a duration", () => {
    const list = [
      make({ name: "short", duration: 60 }),
      make({ name: "mid", duration: 240 }),
      make({ name: "long", duration: 600 }),
      make({ name: "nodur" }),
    ];
    expect(names(applyFilters(list, withFilters({ durationSecs: { min: 120, max: 300 } })))).toEqual(["mid"]);
    expect(names(applyFilters(list, withFilters({ durationSecs: { max: 240 } })))).toEqual(["short", "mid"]);
    expect(names(applyFilters(list, withFilters({ durationSecs: {} })))).toHaveLength(4);
  });

  it("text filter is case and diacritics insensitive over name, band and album", () => {
    const list = [
      make({ name: "Crazy in Love", band_name: "Beyoncé" }),
      make({ name: "Other", band_name: "Someone", album_name: "Dangerously in LOVE" }),
      make({ name: "Unrelated", band_name: "Nobody" }),
    ];
    expect(names(applyFilters(list, withFilters({ text: "beyonce" })))).toEqual(["Crazy in Love"]);
    expect(names(applyFilters(list, withFilters({ text: "BEYONCÉ" })))).toEqual(["Crazy in Love"]);
    expect(names(applyFilters(list, withFilters({ text: "dangerously in love" })))).toEqual(["Other"]);
    expect(names(applyFilters(list, withFilters({ text: "in love" })))).toEqual(["Crazy in Love", "Other"]);
  });

  it("empty or whitespace text is no filter, no match returns an empty list", () => {
    const list = [make({ name: "a" }), make({ name: "b" })];
    expect(applyFilters(list, withFilters({ text: "" }))).toHaveLength(2);
    expect(applyFilters(list, withFilters({ text: "   " }))).toHaveLength(2);
    expect(applyFilters(list, withFilters({ text: "zzz" }))).toEqual([]);
  });

  it("text made only of punctuation matches the raw text (band named !!!)", () => {
    const list = [make({ name: "Yeah", band_name: "!!!" }), make({ name: "Other", band_name: "Band" })];
    expect(names(applyFilters(list, withFilters({ text: "!!!" })))).toEqual(["Yeah"]);
  });

  it("combines all filters with AND", () => {
    const list = [
      make({ name: "keep", platform: "youtube", year: 2002, duration: 200 }),
      make({ name: "wrong platform", platform: "bandcamp", year: 2002, duration: 200 }),
      make({ name: "wrong year", platform: "youtube", year: 1990, duration: 200 }),
      make({ name: "wrong text", platform: "youtube", year: 2002, duration: 200, band_name: "x" }),
    ];
    const result = applyFilters(
      list,
      withFilters({
        platform: "youtube",
        year: { min: 2000 },
        durationSecs: { max: 300 },
        text: "keep",
      }),
    );
    expect(names(result)).toEqual(["keep"]);
  });

  it("does not mutate its input", () => {
    const list = [make({ name: "a", year: 1 }), make({ name: "b", year: 2 })];
    const copy = [...list];
    applyFilters(list, withFilters({ year: { min: 2 } }));
    expect(list).toEqual(copy);
  });
});

describe("popularityOf", () => {
  it("uses view_count for tracks and subscriber_count for artists", () => {
    expect(popularityOf(make({ item_type: ItemType.Track, view_count: 500 }))).toBe(500);
    expect(popularityOf(make({ item_type: ItemType.Artist, subscriber_count: 7 }))).toBe(7);
  });

  it("is undefined for albums even when they carry a count", () => {
    expect(popularityOf(make({ item_type: ItemType.Album, view_count: 9, subscriber_count: 9 }))).toBeUndefined();
  });

  it("does not fall back across fields and treats null as unknown", () => {
    expect(popularityOf(make({ item_type: ItemType.Track, subscriber_count: 99 }))).toBeUndefined();
    const nullCount = { ...make({ item_type: ItemType.Track }), view_count: null } as unknown as SearchSuggestion;
    expect(popularityOf(nullCount)).toBeUndefined();
  });
});

describe("sortItems", () => {
  it("relevance asc keeps backend order, desc reverses, both return new arrays", () => {
    const list = [make({ name: "1" }), make({ name: "2" }), make({ name: "3" })];
    const asc = sortItems(list, { key: "relevance", dir: "asc" });
    const desc = sortItems(list, { key: "relevance", dir: "desc" });
    expect(names(asc)).toEqual(["1", "2", "3"]);
    expect(names(desc)).toEqual(["3", "2", "1"]);
    expect(asc).not.toBe(list);
    expect(names(list)).toEqual(["1", "2", "3"]);
  });

  it("sorts by name case and diacritics insensitively", () => {
    const list = [make({ name: "Zebra" }), make({ name: "Éclair" }), make({ name: "apple" })];
    expect(names(sortItems(list, { key: "name", dir: "asc" }))).toEqual(["apple", "Éclair", "Zebra"]);
    expect(names(sortItems(list, { key: "name", dir: "desc" }))).toEqual(["Zebra", "Éclair", "apple"]);
  });

  it("sorts by year both ways with missing years last in both directions", () => {
    const list = [
      make({ name: "none" }),
      make({ name: "2010", year: 2010 }),
      make({ name: "1990", year: 1990 }),
    ];
    expect(names(sortItems(list, { key: "year", dir: "asc" }))).toEqual(["1990", "2010", "none"]);
    expect(names(sortItems(list, { key: "year", dir: "desc" }))).toEqual(["2010", "1990", "none"]);
  });

  it("sorts by duration both ways with missing durations last", () => {
    const list = [
      make({ name: "nodur" }),
      make({ name: "3min", duration: 180 }),
      make({ name: "1min", duration: 60 }),
    ];
    expect(names(sortItems(list, { key: "duration", dir: "asc" }))).toEqual(["1min", "3min", "nodur"]);
    expect(names(sortItems(list, { key: "duration", dir: "desc" }))).toEqual(["3min", "1min", "nodur"]);
  });

  it("sorts tracks by view_count and artists by subscriber_count, missing last", () => {
    const tracks = [
      make({ name: "none" }),
      make({ name: "low", view_count: 10 }),
      make({ name: "high", view_count: 1000 }),
    ];
    expect(names(sortItems(tracks, { key: "popularity", dir: "desc" }))).toEqual(["high", "low", "none"]);
    expect(names(sortItems(tracks, { key: "popularity", dir: "asc" }))).toEqual(["low", "high", "none"]);

    const artists = [
      make({ name: "a2", item_type: ItemType.Artist, subscriber_count: 2 }),
      make({ name: "a0", item_type: ItemType.Artist }),
      make({ name: "a9", item_type: ItemType.Artist, subscriber_count: 9 }),
    ];
    expect(names(sortItems(artists, { key: "popularity", dir: "desc" }))).toEqual(["a9", "a2", "a0"]);
  });

  it("albums have no popularity, so the order is kept in both directions", () => {
    const albums = [
      make({ name: "x", item_type: ItemType.Album, view_count: 5 }),
      make({ name: "y", item_type: ItemType.Album, view_count: 50 }),
    ];
    expect(names(sortItems(albums, { key: "popularity", dir: "asc" }))).toEqual(["x", "y"]);
    expect(names(sortItems(albums, { key: "popularity", dir: "desc" }))).toEqual(["x", "y"]);
  });

  it("is stable for equal keys in both directions", () => {
    const list = [
      make({ name: "first", year: 2000 }),
      make({ name: "second", year: 2000 }),
      make({ name: "third", year: 1999 }),
    ];
    expect(names(sortItems(list, { key: "year", dir: "asc" }))).toEqual(["third", "first", "second"]);
    expect(names(sortItems(list, { key: "year", dir: "desc" }))).toEqual(["first", "second", "third"]);
  });

  it("puts items with an empty name last", () => {
    const list = [make({ name: "" }), make({ name: "b" }), make({ name: "a" })];
    expect(names(sortItems(list, { key: "name", dir: "asc" }))).toEqual(["a", "b", ""]);
  });

  it("handles an empty list", () => {
    expect(sortItems([], { key: "year", dir: "asc" })).toEqual([]);
  });
});

describe("pickProfileArtist", () => {
  const artist = (name: string, over: Partial<SearchSuggestion> = {}) =>
    make({ item_type: ItemType.Artist, name, item_url_path: `https://youtube.com/browse/${name}`, ...over });

  it("picks an exact name match (case and diacritics insensitive)", () => {
    const daft = artist("Daft Punk");
    expect(pickProfileArtist("DAFT PUNK!", [artist("Other"), daft])).toBe(daft);
    const band = artist("Motörhead");
    expect(pickProfileArtist("motorhead", [band])).toBe(band);
  });

  it("picks an artist whose name starts the query followed by a space", () => {
    const daft = artist("Daft Punk");
    expect(pickProfileArtist("daft punk one more time", [daft])).toBe(daft);
  });

  it("prefers an exact match over an earlier prefix candidate", () => {
    const shortName = artist("Daft");
    const exact = artist("Daft Punk");
    expect(pickProfileArtist("daft punk", [shortName, exact])).toBe(exact);
  });

  it("uses the earliest candidate when several are equally good", () => {
    const first = artist("Daft Punk", { item_url_path: "https://a" });
    const second = artist("Daft Punk", { item_url_path: "https://b" });
    expect(pickProfileArtist("daft punk", [first, second])).toBe(first);
  });

  it("matches a prefix of 3 or more characters (name starts with the query)", () => {
    const daft = artist("Daft Punk");
    expect(pickProfileArtist("daf", [daft])).toBe(daft);
  });

  it("does not match a prefix shorter than 3 characters", () => {
    expect(pickProfileArtist("da", [artist("Daft Punk")])).toBeNull();
  });

  it("returns null when nothing matches or the input is empty", () => {
    expect(pickProfileArtist("one more time", [artist("Daft Punk")])).toBeNull();
    expect(pickProfileArtist("", [artist("Daft Punk")])).toBeNull();
    expect(pickProfileArtist("   ", [artist("Daft Punk")])).toBeNull();
    expect(pickProfileArtist("daft punk", [])).toBeNull();
  });

  it("ignores non-artist items and artists without a name", () => {
    const track = make({ item_type: ItemType.Track, name: "Daft Punk" });
    expect(pickProfileArtist("daft punk", [track])).toBeNull();
    expect(pickProfileArtist("daft punk", [artist("")])).toBeNull();
  });

  it("requires a word boundary after the artist name (no partial word match)", () => {
    expect(pickProfileArtist("daft punkish", [artist("Daft Punk")])).toBeNull();
  });
});

describe("mergePages", () => {
  it("appends new items without mutating either input", () => {
    const existing = [make({ name: "a" })];
    const incoming = [make({ name: "b" })];
    const merged = mergePages(existing, incoming);
    expect(names(merged)).toEqual(["a", "b"]);
    expect(merged).not.toBe(existing);
    expect(names(existing)).toEqual(["a"]);
    expect(names(incoming)).toEqual(["b"]);
  });

  it("skips items whose url already exists", () => {
    const shared = make({ name: "shared" });
    const merged = mergePages([shared], [{ ...shared, name: "renamed" }, make({ name: "new" })]);
    expect(names(merged)).toEqual(["shared", "new"]);
  });

  it("skips duplicates inside the incoming page too", () => {
    const dup = make({ name: "dup" });
    const merged = mergePages([], [dup, { ...dup }]);
    expect(merged).toHaveLength(1);
  });

  it("dedupes url-less items by item type, name and band", () => {
    const existing = [make({ name: "Song", band_name: "Band", item_url_path: "" })];
    const incoming = [
      make({ name: "song", band_name: "BAND", item_url_path: "" }),
      make({ name: "Song", band_name: "Other", item_url_path: "" }),
      make({ name: "Song", band_name: "Band", item_type: ItemType.Album, item_url_path: "" }),
    ];
    const merged = mergePages(existing, incoming);
    expect(merged).toHaveLength(3);
    expect(merged[1].band_name).toBe("Other");
    expect(merged[2].item_type).toBe(ItemType.Album);
  });

  it("skips a url-less item that matches a name+band of an existing item", () => {
    const existing = [make({ name: "Song", band_name: "Band" })];
    const merged = mergePages(existing, [make({ name: "Song", band_name: "Band", item_url_path: "" })]);
    expect(merged).toHaveLength(1);
  });

  it("keeps distinct url-less items", () => {
    const merged = mergePages([], [
      make({ name: "a", item_url_path: "" }),
      make({ name: "b", item_url_path: "" }),
    ]);
    expect(names(merged)).toEqual(["a", "b"]);
  });

  it("handles two empty lists", () => {
    expect(mergePages([], [])).toEqual([]);
  });
});
