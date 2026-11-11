import { describe, expect, it } from "vitest";
import { albumStateFromTracks } from "./libraryIndex.ts";

describe("albumStateFromTracks", () => {
  it("is none for no tracks and for none owned", () => {
    expect(albumStateFromTracks([])).toBe("none");
    expect(albumStateFromTracks(["none", "none"])).toBe("none");
  });
  it("is downloaded when all tracks are owned", () => {
    expect(albumStateFromTracks(["downloaded", "downloaded"])).toBe("downloaded");
  });
  it("is partial when some tracks are owned", () => {
    expect(albumStateFromTracks(["downloaded", "none"])).toBe("partial");
  });
});
