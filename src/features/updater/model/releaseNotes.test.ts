import { describe, expect, it } from "vitest";
import { parseReleaseNotes } from "@/features/updater/model/releaseNotes.ts";

const GENERATED =
  "## What's Changed\r\n" +
  "* fix(ui): app-coloured sliders by @seafoodd in https://github.com/seafoodd/blacktape-desktop/pull/65\r\n" +
  "* chore: version 0.5.1 by @seafoodd in https://github.com/seafoodd/blacktape-desktop/pull/66\r\n" +
  "\r\n\r\n" +
  "**Full Changelog**: https://github.com/seafoodd/blacktape-desktop/compare/v0.5.0...v0.5.1";

describe("parseReleaseNotes", () => {
  it("turns GitHub's generated notes into a heading and short items", () => {
    expect(parseReleaseNotes(GENERATED)).toEqual([
      { kind: "heading", text: "What's Changed" },
      { kind: "item", text: "fix(ui): app-coloured sliders (#65)" },
      { kind: "item", text: "chore: version 0.5.1 (#66)" },
    ]);
  });

  it("keeps plain text and strips bold markers", () => {
    expect(parseReleaseNotes("Some **important** fix")).toEqual([
      { kind: "text", text: "Some important fix" },
    ]);
  });

  it("keeps an item that has no PR link as it is", () => {
    expect(parseReleaseNotes("- a change by hand")).toEqual([
      { kind: "item", text: "a change by hand" },
    ]);
  });

  it("returns nothing for blank notes", () => {
    expect(parseReleaseNotes(" \n\n ")).toEqual([]);
  });
});
