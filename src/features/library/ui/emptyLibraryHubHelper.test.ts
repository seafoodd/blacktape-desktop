import { describe, it, expect } from "vitest";
import {
  INSPIRATION_TAGS,
  cleanSearchQuery,
  shouldShowEmptyHub,
  formatFolderScanningLabel,
} from "./emptyLibraryHubHelper.ts";

describe("emptyLibraryHubHelper", () => {
  it("provides popular inspiration tags", () => {
    expect(INSPIRATION_TAGS.length).toBeGreaterThanOrEqual(4);
    expect(INSPIRATION_TAGS).toContain("Electronic");
    expect(INSPIRATION_TAGS).toContain("Aphex Twin");
  });

  it("cleans search queries by trimming whitespace", () => {
    expect(cleanSearchQuery("  Daft Punk  ")).toBe("Daft Punk");
    expect(cleanSearchQuery("")).toBe("");
    expect(cleanSearchQuery("   ")).toBe("");
  });

  describe("shouldShowEmptyHub", () => {
    it("returns true when there are no tabs, no albums, even if stale tab is selected", () => {
      expect(shouldShowEmptyHub(0, 0, "")).toBe(true);
      expect(shouldShowEmptyHub(0, 0, "   ")).toBe(true);
      expect(shouldShowEmptyHub(0, 0, "Daft Punk")).toBe(true);
    });

    it("returns false if tabs exist in the library", () => {
      expect(shouldShowEmptyHub(3, 0, "")).toBe(false);
      expect(shouldShowEmptyHub(5, 2, "Radiohead")).toBe(false);
    });

    it("returns false if albums exist in the library", () => {
      expect(shouldShowEmptyHub(0, 2, "")).toBe(false);
      expect(shouldShowEmptyHub(0, 2, "Radiohead")).toBe(false);
    });

    it("returns false while an album or track download is active", () => {
      expect(shouldShowEmptyHub(0, 0, "", true)).toBe(false);
      expect(shouldShowEmptyHub(0, 0, "The Doors", true)).toBe(false);
    });
  });

  describe("formatFolderScanningLabel", () => {
    it("returns default action label when not scanning", () => {
      expect(formatFolderScanningLabel(false)).toBe("Choose Music Folder");
      expect(formatFolderScanningLabel(false, 100)).toBe("Choose Music Folder");
    });

    it("returns scanning label when scanning without count", () => {
      expect(formatFolderScanningLabel(true)).toBe("Scanning folder…");
      expect(formatFolderScanningLabel(true, 0)).toBe("Scanning folder…");
    });

    it("returns progress scanning label when scanned count is available", () => {
      expect(formatFolderScanningLabel(true, 42)).toBe(
        "Scanning folder (42 songs)…",
      );
    });
  });
});
