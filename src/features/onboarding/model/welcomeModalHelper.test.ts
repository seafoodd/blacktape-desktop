import { describe, it, expect } from "vitest";
import {
  formatFolderDisplay,
  stepTitle,
  stepSubtitle,
  stepBadge,
  PRIVACY_BULLETS,
} from "./welcomeModalHelper.ts";

describe("welcomeModalHelper", () => {
  it("formats folder display correctly", () => {
    expect(formatFolderDisplay(null)).toBe("No music folder selected");
    expect(formatFolderDisplay("")).toBe("No music folder selected");
    expect(formatFolderDisplay("C:\\Music")).toBe("C:\\Music");
  });

  it("returns appropriate titles, subtitles, and badges for steps", () => {
    expect(stepTitle(1)).toBe("Welcome to Blacktape");
    expect(stepTitle(2)).toBe("Help Improve Blacktape");

    expect(stepSubtitle(1)).toBe("Fast, local music player and streamer");
    expect(stepSubtitle(2)).toBe("Crash reports & anonymous diagnostics");

    expect(stepBadge(1)).toContain("Step 1 of 3");
    expect(stepBadge(2)).toContain("Step 2 of 3");
    expect(stepBadge(3)).toBe("Step 3 of 3 · Downloads");
    expect(stepTitle(3)).toBe("Default Download Quality");
  });

  it("provides privacy bullet points", () => {
    expect(PRIVACY_BULLETS.length).toBeGreaterThanOrEqual(3);
    expect(PRIVACY_BULLETS[0]).toContain("Randomized install ID");
  });
});
