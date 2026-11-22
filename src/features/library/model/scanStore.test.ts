import { describe, it, expect, beforeEach } from "vitest";
import { useScanStore, scanBannerText } from "./scanStore.ts";

describe("scanStore", () => {
  beforeEach(() => {
    useScanStore.getState().reset();
  });

  it("initializes with idle phase", () => {
    expect(useScanStore.getState().progress.phase).toBe("idle");
  });

  it("updates progress", () => {
    useScanStore.getState().setProgress({
      phase: "scanning",
      scanned: 50,
      total: 100,
    });
    expect(useScanStore.getState().progress.scanned).toBe(50);
    expect(useScanStore.getState().progress.total).toBe(100);
  });

  it("formats banner text appropriately", () => {
    expect(scanBannerText({ phase: "idle", scanned: 0, total: 0 })).toBeNull();
    expect(
      scanBannerText({ phase: "scanning", scanned: 45, total: 100 }),
    ).toBe("Scanning library… 45 / 100 songs (45%)");
    expect(
      scanBannerText({ phase: "done", scanned: 100, total: 100 }),
    ).toBe("Library scan complete (100 songs)");
  });
});
