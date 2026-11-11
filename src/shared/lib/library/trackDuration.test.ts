import { describe, expect, it } from "vitest";
import { formatTotalDuration, formatTrackDuration } from "./trackDuration.ts";

describe("formatTrackDuration", () => {
  it("formats minutes and seconds", () => {
    expect(formatTrackDuration(185000)).toBe("3:05");
    expect(formatTrackDuration(55000)).toBe("0:55");
  });
  it("formats hours", () => {
    expect(formatTrackDuration(3723000)).toBe("1:02:03");
  });
  it("is empty for unknown values", () => {
    expect(formatTrackDuration(null)).toBe("");
    expect(formatTrackDuration(undefined)).toBe("");
    expect(formatTrackDuration(0)).toBe("");
    expect(formatTrackDuration(Number.NaN)).toBe("");
  });
});

describe("formatTotalDuration", () => {
  it("formats minutes, hours and seconds", () => {
    expect(formatTotalDuration(42 * 60000)).toBe("42 min");
    expect(formatTotalDuration(65 * 60000)).toBe("1 hr 5 min");
    expect(formatTotalDuration(2 * 3600000)).toBe("2 hr");
    expect(formatTotalDuration(30000)).toBe("30 sec");
  });
  it("is empty for unknown values", () => {
    expect(formatTotalDuration(null)).toBe("");
    expect(formatTotalDuration(-5)).toBe("");
  });
});
