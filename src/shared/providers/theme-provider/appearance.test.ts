import { describe, expect, it } from "vitest";
import {
  DEFAULT_BG_BLUR,
  DEFAULT_BG_OPACITY,
  hexToRgb,
  parseBlurSetting,
  parseUnitSetting,
  windowBlurEnabled,
} from "@/shared/providers/theme-provider/appearance.ts";

describe("defaults", () => {
  it("start fully opaque with no blur", () => {
    expect(DEFAULT_BG_OPACITY).toBe(1);
    expect(DEFAULT_BG_BLUR).toBe(false);
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

describe("parseBlurSetting", () => {
  it("gives the fallback when nothing is stored", () => {
    expect(parseBlurSetting(null, false)).toBe(false);
    expect(parseBlurSetting(null, true)).toBe(true);
  });

  it("reads true and false, ignoring surrounding whitespace", () => {
    expect(parseBlurSetting("true", false)).toBe(true);
    expect(parseBlurSetting(" false ", true)).toBe(false);
  });

  it("reads an old slider value: above 0 is on, 0 is off", () => {
    expect(parseBlurSetting("0.5", false)).toBe(true);
    expect(parseBlurSetting("0", true)).toBe(false);
  });

  it("gives the fallback for empty or unknown text", () => {
    expect(parseBlurSetting("", true)).toBe(true);
    expect(parseBlurSetting("yes", false)).toBe(false);
    expect(parseBlurSetting("NaN", true)).toBe(true);
  });
});

describe("windowBlurEnabled", () => {
  it("is off at full opacity even with blur on", () => {
    expect(windowBlurEnabled(1, true)).toBe(false);
  });

  it("is off with blur off, even when see-through", () => {
    expect(windowBlurEnabled(0.5, false)).toBe(false);
  });

  it("is on when see-through and blur is on", () => {
    expect(windowBlurEnabled(0.5, true)).toBe(true);
    expect(windowBlurEnabled(0, true)).toBe(true);
  });
});

describe("hexToRgb", () => {
  it("reads six-digit colours, with or without #, in any case", () => {
    expect(hexToRgb("#0F1015")).toEqual([15, 16, 21]);
    expect(hexToRgb("0f1015")).toEqual([15, 16, 21]);
  });

  it("expands three-digit colours", () => {
    expect(hexToRgb("#fa0")).toEqual([255, 170, 0]);
  });

  it("gives black for anything that is not a hex colour", () => {
    expect(hexToRgb("")).toEqual([0, 0, 0]);
    expect(hexToRgb("rgb(1, 2, 3)")).toEqual([0, 0, 0]);
    expect(hexToRgb("#12345")).toEqual([0, 0, 0]);
    expect(hexToRgb("#gggggg")).toEqual([0, 0, 0]);
  });
});
