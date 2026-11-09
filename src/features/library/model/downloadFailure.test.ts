import { describe, it, expect } from "vitest";
import { explainFailure } from "./downloadFailure.ts";

describe("explainFailure", () => {
  it("explains a song with no stream", () => {
    const e = explainFailure("Download failed: ERROR: [Bandcamp] 1: No video formats found!", "Bandcamp");
    expect(e.kind).toBe("no-stream");
    expect(e.reason).toContain("Bandcamp");
  });
  it("explains a blocked or age-restricted song", () => {
    expect(explainFailure("ERROR: [youtube] x: Requested format is not available", "YouTube").kind).toBe("restricted");
    expect(explainFailure("This video is age-restricted", "YouTube").kind).toBe("restricted");
  });
  it("explains a network problem as retryable", () => {
    expect(explainFailure("HTTP Error 403: Forbidden", "YouTube").kind).toBe("network");
  });
  it("falls back to the error text, shortened", () => {
    const e = explainFailure("Download failed: weird " + "x".repeat(500), "YouTube");
    expect(e.kind).toBe("other");
    expect(e.reason.startsWith("weird")).toBe(true);
    expect(e.reason.length).toBeLessThanOrEqual(240);
  });
});
