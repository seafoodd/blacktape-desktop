import { describe, it, expect } from "vitest";
import { fileFormat, songQuality } from "./songQuality.ts";

describe("fileFormat", () => {
  it("reads the format from a Windows path", () => {
    expect(fileFormat("C:\\lib\\A\\01 - x.flac")).toBe("FLAC");
  });

  it("reads the format from a posix path", () => {
    expect(fileFormat("/a/b.mp3")).toBe("MP3");
  });

  it("uppercases a mixed-case extension", () => {
    expect(fileFormat("/music/track.Opus")).toBe("OPUS");
  });

  it("takes only the part after the last dot when the name has several dots", () => {
    expect(fileFormat("a.b.m4a")).toBe("M4A");
  });

  it("gives null when the file has no extension", () => {
    expect(fileFormat("/a/b/song")).toBeNull();
  });

  it("gives null for a trailing dot", () => {
    expect(fileFormat("x.")).toBeNull();
  });

  it("gives null for a dotfile", () => {
    expect(fileFormat(".hidden")).toBeNull();
  });

  it("ignores dots in directory names", () => {
    expect(fileFormat("C:\\a.b\\song")).toBeNull();
  });
});

describe("songQuality", () => {
  it("labels a low-quality mp3 as MP3 · Low", () => {
    expect(songQuality({ path: "/m/a.mp3", quality_tier: "low" })).toEqual({
      tier: "low",
      label: "MP3 · Low",
      title: "Quality: Low (MP3)",
    });
  });

  it("labels a high-quality opus as OPUS · High", () => {
    expect(songQuality({ path: "/m/a.opus", quality_tier: "high" })).toEqual({
      tier: "high",
      label: "OPUS · High",
      title: "Quality: High (OPUS)",
    });
  });

  it("labels a standard m4a as M4A · Standard", () => {
    expect(songQuality({ path: "/m/a.m4a", quality_tier: "standard" })).toEqual({
      tier: "standard",
      label: "M4A · Standard",
      title: "Quality: Standard (M4A)",
    });
  });

  it("labels a lossless flac as just FLAC", () => {
    expect(songQuality({ path: "C:\\lib\\A\\01 - x.flac", quality_tier: "lossless" })).toEqual({
      tier: "lossless",
      label: "FLAC",
      title: "Quality: Lossless (FLAC)",
    });
  });

  it("labels a lossless m4a as ALAC", () => {
    expect(songQuality({ path: "/m/a.m4a", quality_tier: "lossless" })).toEqual({
      tier: "lossless",
      label: "ALAC",
      title: "Quality: Lossless (ALAC)",
    });
  });

  it("labels a lossless mp4 as ALAC", () => {
    expect(songQuality({ path: "/m/a.mp4", quality_tier: "lossless" })).toEqual({
      tier: "lossless",
      label: "ALAC",
      title: "Quality: Lossless (ALAC)",
    });
  });

  it("keeps the M4A label for a high-quality m4a", () => {
    expect(songQuality({ path: "/m/a.m4a", quality_tier: "high" })).toEqual({
      tier: "high",
      label: "M4A · High",
      title: "Quality: High (M4A)",
    });
  });

  it("keeps the FLAC label for a lossless flac", () => {
    expect(songQuality({ path: "/m/a.flac", quality_tier: "lossless" })).toEqual({
      tier: "lossless",
      label: "FLAC",
      title: "Quality: Lossless (FLAC)",
    });
  });

  it("uses only the tier word when the file has no extension", () => {
    expect(songQuality({ path: "/m/song", quality_tier: "high" })).toEqual({
      tier: "high",
      label: "High",
      title: "Quality: High",
    });
  });

  it("falls back to the low tier for an unknown tier value", () => {
    const result = songQuality({ path: "/m/a.mp3", quality_tier: "bogus" as never });
    expect(result.tier).toBe("low");
    expect(result.label).toBe("MP3 · Low");
    expect(result.title).toBe("Quality: Low (MP3)");
  });
});
