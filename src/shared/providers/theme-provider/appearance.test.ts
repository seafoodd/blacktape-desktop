import { describe, expect, it } from "vitest";
import {
  DEFAULT_BG_BLUR,
  DEFAULT_BG_OPACITY,
  parseUnitSetting,
  windowBlurEnabled,
} from "@/shared/providers/theme-provider/appearance.ts";

describe("defaults", () => {
  it("start fully opaque with no blur", () => {
    expect(DEFAULT_BG_OPACITY).toBe(1);
    expect(DEFAULT_BG_BLUR).toBe(0);
  });
});

describe("parseUnitSetting", () => {
  it("gives the fallback when nothing is stored", () => {
    expect(parseUnitSetting(null, 0.7)).toBe(0.7);
  });

  it("gives the fallback for empty or whitespace-only text", () => {
    expect(parseUnitSetting("", 0.7)).toBe(0.7);
    expect(parseUnitSetting("   ", 0.7)).toBe(0.7);
  });

  it("gives the fallback for non-numeric text", () => {
    expect(parseUnitSetting("abc", 0.7)).toBe(0.7);
    expect(parseUnitSetting("NaN", 0.7)).toBe(0.7);
    expect(parseUnitSetting("Infinity", 0.7)).toBe(0.7);
  });

  it("parses a number inside 0-1", () => {
    expect(parseUnitSetting("0.5", 1)).toBe(0.5);
  });

  it("keeps 0 as a value instead of using the fallback", () => {
    expect(parseUnitSetting("0", 1)).toBe(0);
  });

  it("keeps 1 as a value", () => {
    expect(parseUnitSetting("1", 0)).toBe(1);
  });

  it("clamps values above 1 to 1", () => {
    expect(parseUnitSetting("1.7", 0)).toBe(1);
  });

  it("clamps negative values to 0", () => {
    expect(parseUnitSetting("-0.3", 1)).toBe(0);
  });

  it("ignores surrounding whitespace around a number", () => {
    expect(parseUnitSetting(" 0.25 ", 1)).toBe(0.25);
  });
});

describe("windowBlurEnabled", () => {
  it("is off at full opacity even with blur", () => {
    expect(windowBlurEnabled(1, 0.5)).toBe(false);
  });

  it("is off with no blur, even when see-through", () => {
    expect(windowBlurEnabled(0.5, 0)).toBe(false);
  });

  it("is on when see-through and blurred", () => {
    expect(windowBlurEnabled(0.5, 0.3)).toBe(true);
  });

  it("is on when fully transparent and blurred", () => {
    expect(windowBlurEnabled(0, 1)).toBe(true);
  });
});
