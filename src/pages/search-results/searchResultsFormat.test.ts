import { describe, it, expect } from "vitest";
import {
  buildDownloadMap,
  searchItemDownloadParams,
  CLEARED_FILTERS,
  downloadPercent,
  formatCountLabel,
  formatDurationCell,
  formatDurationField,
  formatDurationSecs,
  formatAudience,
  formatPlays,
  formatSubscribers,
  hasActiveFilters,
  isDownloadBusy,
  joinMeta,
  parseDurationInput,
  parseYearInput,
  platformLabel,
  trackAlbumItem,
  trackSubline,
  uniqueWarnings,
} from "./searchResultsFormat.ts";
import { ItemType, type DownloadProgress } from "@/shared/store/libraryStore.ts";

const download = (url: string, status: DownloadProgress["status"]): DownloadProgress =>
  ({ url, status }) as DownloadProgress;

describe("formatDurationSecs", () => {
  it("formats m:ss below an hour", () => {
    expect(formatDurationSecs(0)).toBe("0:00");
    expect(formatDurationSecs(61)).toBe("1:01");
    expect(formatDurationSecs(3599)).toBe("59:59");
  });

  it("switches to h:mm:ss from an hour on", () => {
    expect(formatDurationSecs(3600)).toBe("1:00:00");
    expect(formatDurationSecs(3723)).toBe("1:02:03");
  });

  it("floors fractional seconds", () => {
    expect(formatDurationSecs(59.9)).toBe("0:59");
  });

  it("renders invalid or negative input as 0:00", () => {
    expect(formatDurationSecs(NaN)).toBe("0:00");
    expect(formatDurationSecs(Infinity)).toBe("0:00");
    expect(formatDurationSecs(-5)).toBe("0:00");
  });
});

describe("formatDurationCell", () => {
  it("formats a known duration like formatDurationSecs", () => {
    expect(formatDurationCell(185)).toBe("3:05");
    expect(formatDurationCell(0)).toBe("0:00");
    expect(formatDurationCell(3723)).toBe("1:02:03");
  });

  it("is an empty placeholder when the duration is unknown or invalid", () => {
    expect(formatDurationCell(undefined)).toBe("");
    expect(formatDurationCell(null)).toBe("");
    expect(formatDurationCell(NaN)).toBe("");
    expect(formatDurationCell(-1)).toBe("");
  });
});

describe("formatDurationField", () => {
  it("is empty for an unset bound and mm:ss otherwise", () => {
    expect(formatDurationField(undefined)).toBe("");
    expect(formatDurationField(210)).toBe("3:30");
  });
});

describe("parseDurationInput", () => {
  it("treats empty input as no bound", () => {
    expect(parseDurationInput("")).toEqual({ value: undefined });
    expect(parseDurationInput("   ")).toEqual({ value: undefined });
  });

  it("reads plain numbers as minutes", () => {
    expect(parseDurationInput("3")).toEqual({ value: 180 });
    expect(parseDurationInput("1.5")).toEqual({ value: 90 });
    expect(parseDurationInput("0")).toEqual({ value: 0 });
  });

  it("reads m:ss and h:mm:ss", () => {
    expect(parseDurationInput("3:30")).toEqual({ value: 210 });
    expect(parseDurationInput("90:00")).toEqual({ value: 5400 });
    expect(parseDurationInput("1:02:03")).toEqual({ value: 3723 });
    expect(parseDurationInput(" 2:00 ")).toEqual({ value: 120 });
  });

  it("rejects seconds or minutes out of range", () => {
    expect(parseDurationInput("3:75")).toBeNull();
    expect(parseDurationInput("1:60:00")).toBeNull();
    expect(parseDurationInput("1:60:30")).toBeNull();
  });

  it("rejects malformed input", () => {
    expect(parseDurationInput("abc")).toBeNull();
    expect(parseDurationInput("-1")).toBeNull();
    expect(parseDurationInput("3:")).toBeNull();
    expect(parseDurationInput(":30")).toBeNull();
    expect(parseDurationInput("1:2:3:4")).toBeNull();
    expect(parseDurationInput("1.2.3")).toBeNull();
  });

  it("round-trips with formatDurationField", () => {
    const parsed = parseDurationInput(formatDurationField(3723));
    expect(parsed).toEqual({ value: 3723 });
  });
});

describe("parseYearInput", () => {
  it("accepts positive integers up to 9999", () => {
    expect(parseYearInput("2013")).toBe(2013);
    expect(parseYearInput(" 1999 ")).toBe(1999);
    expect(parseYearInput("9999")).toBe(9999);
  });

  it("treats empty or invalid input as no bound", () => {
    expect(parseYearInput("")).toBeUndefined();
    expect(parseYearInput("abc")).toBeUndefined();
    expect(parseYearInput("0")).toBeUndefined();
    expect(parseYearInput("-5")).toBeUndefined();
    expect(parseYearInput("2013.5")).toBeUndefined();
    expect(parseYearInput("10000")).toBeUndefined();
  });
});

describe("platformLabel", () => {
  it("labels known platforms", () => {
    expect(platformLabel("youtube")).toBe("YouTube");
    expect(platformLabel("bandcamp")).toBe("Bandcamp");
    expect(platformLabel("local")).toBe("Local");
  });

  it("passes unknown values through and tolerates missing ones", () => {
    expect(platformLabel("tidal")).toBe("tidal");
    expect(platformLabel(null)).toBe("");
    expect(platformLabel(undefined)).toBe("");
  });
});

describe("downloadPercent", () => {
  it("rounds the overall percent", () => {
    expect(downloadPercent({ overallPercent: 42.6, current: 0, total: 0 })).toBe(43);
  });

  it("clamps the overall percent to 0..100", () => {
    expect(downloadPercent({ overallPercent: 150, current: 0, total: 0 })).toBe(100);
    expect(downloadPercent({ overallPercent: -5, current: 0, total: 0 })).toBe(0);
  });

  it("falls back to current/total when no overall percent is known", () => {
    expect(
      downloadPercent({ overallPercent: undefined as unknown as number, current: 1, total: 4 }),
    ).toBe(25);
    expect(
      downloadPercent({ overallPercent: NaN, current: 1, total: 3 }),
    ).toBe(33);
  });

  it("is 0 while nothing is known", () => {
    expect(downloadPercent({ overallPercent: 0, current: 5, total: 0 })).toBe(0);
    expect(downloadPercent({ overallPercent: 0, current: 0, total: 4 })).toBe(0);
  });
});

describe("isDownloadBusy", () => {
  it("is false without a download and for failed or cancelled ones", () => {
    expect(isDownloadBusy(undefined)).toBe(false);
    expect(isDownloadBusy({ status: "failed" })).toBe(false);
    expect(isDownloadBusy({ status: "cancelled" })).toBe(false);
  });

  it("is true while running and for completed (shows the check)", () => {
    expect(isDownloadBusy({ status: "idle" })).toBe(true);
    expect(isDownloadBusy({ status: "downloading" })).toBe(true);
    expect(isDownloadBusy({ status: "processing" })).toBe(true);
    expect(isDownloadBusy({ status: "completed" })).toBe(true);
  });
});

describe("buildDownloadMap", () => {
  it("is empty for an empty list", () => {
    expect(buildDownloadMap([]).size).toBe(0);
  });

  it("indexes downloads by url", () => {
    const map = buildDownloadMap([download("a", "downloading"), download("b", "completed")]);
    expect(map.get("a")?.status).toBe("downloading");
    expect(map.get("b")?.status).toBe("completed");
    expect(map.get("c")).toBeUndefined();
  });

  it("prefers a busy download over a failed one for the same url, in either order", () => {
    const a = buildDownloadMap([download("u", "failed"), download("u", "downloading")]);
    expect(a.get("u")?.status).toBe("downloading");
    const b = buildDownloadMap([download("u", "downloading"), download("u", "failed")]);
    expect(b.get("u")?.status).toBe("downloading");
  });

  it("keeps the first download when none is busy", () => {
    const map = buildDownloadMap([download("u", "failed"), download("u", "cancelled")]);
    expect(map.get("u")?.status).toBe("failed");
  });
});

describe("hasActiveFilters", () => {
  it("is false for the cleared filters", () => {
    expect(hasActiveFilters(CLEARED_FILTERS)).toBe(false);
  });

  it("is false for whitespace-only text and empty ranges", () => {
    expect(
      hasActiveFilters({ platform: "All", year: {}, durationSecs: {}, text: "   " }),
    ).toBe(false);
  });

  it("is true for each active filter", () => {
    const base = CLEARED_FILTERS;
    expect(hasActiveFilters({ ...base, platform: "youtube" })).toBe(true);
    expect(hasActiveFilters({ ...base, year: { min: 2000 } })).toBe(true);
    expect(hasActiveFilters({ ...base, year: { max: 2010 } })).toBe(true);
    expect(hasActiveFilters({ ...base, durationSecs: { max: 300 } })).toBe(true);
    expect(hasActiveFilters({ ...base, text: "x" })).toBe(true);
  });
});

describe("formatCountLabel", () => {
  it("adds a plus while more pages exist", () => {
    expect(formatCountLabel(12, false)).toBe("12");
    expect(formatCountLabel(12, true)).toBe("12+");
    expect(formatCountLabel(0, false)).toBe("0");
  });
});

describe("uniqueWarnings", () => {
  it("drops empty and duplicate messages, keeps order", () => {
    expect(uniqueWarnings(["a", "a", " ", "b", "a "])).toEqual(["a", "b"]);
  });

  it("handles an empty list", () => {
    expect(uniqueWarnings([])).toEqual([]);
  });
});

describe("joinMeta", () => {
  it("joins present parts with a middle dot", () => {
    expect(joinMeta(["a", null, undefined, false, "", "b"])).toBe("a · b");
  });

  it("is empty when nothing is present", () => {
    expect(joinMeta([null, undefined, false])).toBe("");
  });
});

describe("formatPlays and formatSubscribers", () => {
  it("omit unknown counts", () => {
    expect(formatPlays(undefined)).toBeNull();
    expect(formatPlays(null)).toBeNull();
    expect(formatSubscribers(undefined)).toBeNull();
    expect(formatSubscribers(0)).toBeNull();
  });

  it("format known counts compactly", () => {
    expect(formatPlays(0)).toBe("0 plays");
    expect(formatPlays(1234)).toBe("1,234 plays");
    expect(formatPlays(1_200_000_000)).toBe("1.2B plays");
    expect(formatSubscribers(7_210_000)).toBe("7.2M subscribers");
  });
});

describe("formatAudience", () => {
  it("labels by kind and never guesses", () => {
    expect(formatAudience(67_400_000, "monthly_audience")).toBe("67.4M monthly audience");
    expect(formatAudience(1_700, "subscribers")).toBe("1,700 subscribers");
    expect(formatAudience(1_700, "followers")).toBe("1,700 followers");
    expect(formatAudience(2_500_000, "followers")).toBe("2.5M followers");
    expect(formatAudience(1_700, null)).toBeNull();
    expect(formatAudience(1_700, undefined)).toBeNull();
    expect(formatAudience(0, "subscribers")).toBeNull();
    expect(formatAudience(undefined, "subscribers")).toBeNull();
  });
});

describe("trackSubline", () => {
  it("joins band and album", () => {
    expect(trackSubline("Daft Punk", "Random Access Memories")).toBe(
      "Daft Punk - Random Access Memories",
    );
  });

  it("skips missing or blank parts", () => {
    expect(trackSubline("Band", null)).toBe("Band");
    expect(trackSubline("", "Album")).toBe("Album");
    expect(trackSubline("  ", "")).toBe("");
  });
});

describe("trackAlbumItem", () => {
  const track = {
    name: "Song",
    band_name: "Artist",
    album_name: "Album",
    album_url: "https://example.com/album",
    img: "https://example.com/img.jpg",
    platform: "youtube" as const,
  };

  it("returns null when the album url is missing, empty or blank", () => {
    expect(trackAlbumItem({ ...track, album_url: undefined })).toBeNull();
    expect(trackAlbumItem({ ...track, album_url: null })).toBeNull();
    expect(trackAlbumItem({ ...track, album_url: "" })).toBeNull();
    expect(trackAlbumItem({ ...track, album_url: "   " })).toBeNull();
  });

  it("returns null when the album name is missing, empty or blank", () => {
    expect(trackAlbumItem({ ...track, album_name: undefined })).toBeNull();
    expect(trackAlbumItem({ ...track, album_name: null })).toBeNull();
    expect(trackAlbumItem({ ...track, album_name: "" })).toBeNull();
    expect(trackAlbumItem({ ...track, album_name: "  " })).toBeNull();
  });

  it("maps a track with an album to an album search item", () => {
    expect(trackAlbumItem(track)).toEqual({
      item_type: ItemType.Album,
      name: "Album",
      band_name: "Artist",
      item_url_path: "https://example.com/album",
      img: "https://example.com/img.jpg",
      platform: "youtube",
    });
  });

  it("trims the album name and url", () => {
    const item = trackAlbumItem({
      ...track,
      album_name: "  Album  ",
      album_url: "  https://example.com/album  ",
    });
    expect(item?.name).toBe("Album");
    expect(item?.item_url_path).toBe("https://example.com/album");
  });

  it("uses the first credited artist when several are listed", () => {
    const item = trackAlbumItem({ ...track, band_name: "A,  B, C" });
    expect(item?.band_name).toBe("A");
  });
});

describe("searchItemDownloadParams", () => {
  const base = {
    name: "Cesare",
    band_name: "Car Seat Headrest, Guest",
    item_url_path: "https://a.test/track/cesare",
    img: "i.jpg",
    platform: "bandcamp" as never,
  };

  it("sends a track with a known album through the album pipeline", () => {
    const p = searchItemDownloadParams({
      ...base,
      item_type: ItemType.Track,
      album_name: "1",
      album_url: "https://a.test/album/1",
    });
    expect(p.downloadType).toBe(ItemType.Album);
    expect(p.url).toBe("https://a.test/album/1");
    expect(p.trackUrls).toEqual(["https://a.test/track/cesare"]);
    expect(p.artistName).toBe("Car Seat Headrest");
  });

  it("downloads a track without an album as a track", () => {
    const p = searchItemDownloadParams({ ...base, item_type: ItemType.Track });
    expect(p.downloadType).toBe(ItemType.Track);
    expect(p.trackUrls).toBeUndefined();
  });
});
