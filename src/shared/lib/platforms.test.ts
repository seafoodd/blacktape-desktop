import { describe, it, expect } from "vitest";
import {
  PLATFORM_FILTERS,
  SEARCH_PLATFORMS,
  canRedownload,
  matchesPlatformFilter,
  platformFilterLabel,
  platformLabel,
} from "./platforms.ts";

describe("platformLabel", () => {
  it("maps soundcloud to SoundCloud", () => {
    expect(platformLabel("soundcloud")).toBe("SoundCloud");
  });

  it("maps youtube to YouTube", () => {
    expect(platformLabel("youtube")).toBe("YouTube");
  });

  it("matches the platform name case-insensitively", () => {
    expect(platformLabel("SoundCloud")).toBe("SoundCloud");
  });

  it("returns unknown values as-is", () => {
    expect(platformLabel("spotify")).toBe("spotify");
  });
});

describe("platformFilterLabel", () => {
  it("labels All as All Platforms", () => {
    expect(platformFilterLabel("All")).toBe("All Platforms");
  });

  it("labels Soundcloud as SoundCloud", () => {
    expect(platformFilterLabel("Soundcloud")).toBe("SoundCloud");
  });
});

describe("matchesPlatformFilter", () => {
  it("All matches any platform", () => {
    expect(matchesPlatformFilter("youtube", "All")).toBe(true);
  });

  it("All matches an undefined platform", () => {
    expect(matchesPlatformFilter(undefined, "All")).toBe(true);
  });

  it("All matches a null platform", () => {
    expect(matchesPlatformFilter(null, "All")).toBe(true);
  });

  it("Soundcloud matches a lowercase soundcloud platform", () => {
    expect(matchesPlatformFilter("soundcloud", "Soundcloud")).toBe(true);
  });

  it("Soundcloud does not match youtube", () => {
    expect(matchesPlatformFilter("youtube", "Soundcloud")).toBe(false);
  });

  it("does not match an undefined platform against a specific filter", () => {
    expect(matchesPlatformFilter(undefined, "Youtube")).toBe(false);
  });
});

describe("canRedownload", () => {
  it("allows soundcloud with a url", () => {
    expect(canRedownload("soundcloud", "https://soundcloud.com/forss/flickermood")).toBe(true);
  });

  it("allows youtube with a url", () => {
    expect(canRedownload("youtube", "https://youtube.com/watch?v=1")).toBe(true);
  });

  it("allows bandcamp with a url", () => {
    expect(canRedownload("bandcamp", "https://artist.bandcamp.com/track/x")).toBe(true);
  });

  it("rejects local files even with a url", () => {
    expect(canRedownload("local", "C:/music/song.mp3")).toBe(false);
  });

  it("rejects a null url", () => {
    expect(canRedownload("soundcloud", null)).toBe(false);
  });

  it("rejects an empty url", () => {
    expect(canRedownload("soundcloud", "")).toBe(false);
  });

  it("rejects an undefined url", () => {
    expect(canRedownload("soundcloud", undefined)).toBe(false);
  });

  it("rejects an undefined source", () => {
    expect(canRedownload(undefined, "https://soundcloud.com/forss/flickermood")).toBe(false);
  });
});

describe("constants", () => {
  it("PLATFORM_FILTERS includes Soundcloud", () => {
    expect(PLATFORM_FILTERS).toContain("Soundcloud");
  });

  it("SEARCH_PLATFORMS includes soundcloud, youtube and bandcamp", () => {
    expect(SEARCH_PLATFORMS).toEqual(expect.arrayContaining(["soundcloud", "youtube", "bandcamp"]));
  });
});
