import { describe, expect, it } from "vitest";
import { getQualityTierBadge } from "./canonical";

describe("canonical types and helpers", () => {
  it("returns correct badges for all quality tiers", () => {
    expect(getQualityTierBadge("lossless").label).toContain("Lossless");
    expect(getQualityTierBadge("high_lossy").label).toContain("High");
    expect(getQualityTierBadge("standard_lossy").label).toContain("Standard");
    expect(getQualityTierBadge("low").label).toContain("Low");
  });
});
