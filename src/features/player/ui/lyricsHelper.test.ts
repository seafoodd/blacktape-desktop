import { describe, it, expect } from "vitest";
import { lyricsEmptyText } from "./lyricsHelper.ts";

describe("lyricsHelper", () => {
  describe("lyricsEmptyText", () => {
    it("returns prompt to play track when there is no active song", () => {
      expect(lyricsEmptyText(false, null)).toBe("Play a track to view lyrics");
      expect(lyricsEmptyText(false, "Some error")).toBe(
        "Play a track to view lyrics",
      );
    });

    it("returns fallback message when active song has no lyrics and no error", () => {
      expect(lyricsEmptyText(true, null)).toBe("No lyrics found.");
      expect(lyricsEmptyText(true, "")).toBe("No lyrics found.");
    });

    it("returns specific error status when active song fetch failed with error", () => {
      expect(lyricsEmptyText(true, "Lyrics service timed out")).toBe(
        "Lyrics service timed out",
      );
    });
  });
});
