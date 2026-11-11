import { describe, it, expect } from "vitest";
import { findDownload, isRunning } from "@/shared/lib/downloadMatch.ts";
import type { DownloadStatus } from "@/shared/store/libraryStore.ts";

type Entry = { url: string; status: DownloadStatus; trackUrls?: string[] };

const ALBUM = "https://a.bandcamp.com/album/x";
const TRACK_1 = "https://a.bandcamp.com/track/t1";
const TRACK_2 = "https://a.bandcamp.com/track/t2";
const TRACK_3 = "https://a.bandcamp.com/track/t3";

function entry(id: string, overrides: Partial<Entry> = {}): Record<string, Entry> {
  return {
    [id]: { url: ALBUM, status: "downloading", ...overrides },
  };
}

describe("findDownload", () => {
  it("finds a download with the same url", () => {
    const active = entry("a");
    expect(findDownload(active, { url: ALBUM })).toBe(active.a);
  });

  it("treats trailing slash, letter case and http/https as the same page", () => {
    const active = entry("a");
    expect(findDownload(active, { url: "https://a.bandcamp.com/album/x/" })).toBe(active.a);
    expect(findDownload(active, { url: "HTTPS://A.BANDCAMP.COM/ALBUM/X" })).toBe(active.a);
    expect(findDownload(active, { url: "http://a.bandcamp.com/album/x" })).toBe(active.a);
  });

  it("ignores failed and cancelled downloads", () => {
    expect(findDownload(entry("a", { status: "failed" }), { url: ALBUM })).toBeUndefined();
    expect(findDownload(entry("a", { status: "cancelled" }), { url: ALBUM })).toBeUndefined();
  });

  it("returns completed, downloading, processing and idle downloads", () => {
    for (const status of ["completed", "downloading", "processing", "idle"] as const) {
      const active = entry("a", { status });
      expect(findDownload(active, { url: ALBUM })).toBe(active.a);
    }
  });

  it("returns undefined for a different url", () => {
    expect(
      findDownload(entry("a"), { url: "https://a.bandcamp.com/album/other" }),
    ).toBeUndefined();
  });

  it("returns undefined when active is empty", () => {
    expect(findDownload({}, { url: ALBUM })).toBeUndefined();
  });

  describe("with trackUrls requested", () => {
    it("matches a download without trackUrls (whole album)", () => {
      const active = entry("a");
      expect(findDownload(active, { url: ALBUM, trackUrls: [TRACK_1] })).toBe(active.a);
    });

    it("matches a download whose trackUrls include all requested ones", () => {
      const active = entry("a", { trackUrls: [TRACK_1, TRACK_2, TRACK_3] });
      expect(findDownload(active, { url: ALBUM, trackUrls: [TRACK_1, TRACK_3] })).toBe(
        active.a,
      );
    });

    it("does not match a download that lacks one of the requested tracks", () => {
      const active = entry("a", { trackUrls: [TRACK_1, TRACK_2] });
      expect(
        findDownload(active, { url: ALBUM, trackUrls: [TRACK_1, TRACK_3] }),
      ).toBeUndefined();
    });
  });

  it("does not let a download of some tracks cover the whole album", () => {
    const active = entry("a", { trackUrls: [TRACK_1] });
    expect(findDownload(active, { url: ALBUM })).toBeUndefined();
  });

  it("prefers a running download over a completed one of the same page", () => {
    const active = {
      ...entry("done", { status: "completed" }),
      ...entry("run", { status: "downloading" }),
    };
    expect(findDownload(active, { url: ALBUM })).toBe(active.run);
  });
});

describe("isRunning", () => {
  it("is false for undefined", () => {
    expect(isRunning(undefined)).toBe(false);
  });

  it.each(["downloading", "processing", "idle"] as const)("is true for %s", (status) => {
    expect(isRunning({ status })).toBe(true);
  });

  it.each(["completed", "failed", "cancelled"] as const)("is false for %s", (status) => {
    expect(isRunning({ status })).toBe(false);
  });
});
