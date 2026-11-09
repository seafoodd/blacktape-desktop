import { describe, it, expect } from "vitest";
import { urlKey, sameUrl } from "./urlKey";

describe("urlKey", () => {
  it("returns null for empty, nullish or whitespace input", () => {
    expect(urlKey(null)).toBeNull();
    expect(urlKey(undefined)).toBeNull();
    expect(urlKey("")).toBeNull();
    expect(urlKey("   ")).toBeNull();
  });

  it("returns null for unparsable or non-web urls", () => {
    expect(urlKey("garbage")).toBeNull();
  });

  // KNOWN BUG: without "://" the input is prefixed with "https://", so "mailto:a@b.com" is parsed as
  // userinfo "mailto:a" + host "b.com" and keyed as "b.com". Expected null. Flips to a failure
  // (remove .fails) once urlKey.ts rejects non-http(s) schemes before prefixing.
  it("returns null for mailto: urls (known bug: keyed as b.com)", () => {
    expect(urlKey("mailto:a@b.com")).toBeNull();
  });

  it("ignores scheme, www./m./music. prefixes, query, fragment and trailing slash", () => {
    const expected = "example.com/path";
    expect(urlKey("https://www.example.com/Path/?a=1#x")).toBe(expected);
    expect(urlKey("http://example.com/path")).toBe(expected);
    expect(urlKey("https://m.example.com/PATH")).toBe(expected);
    expect(urlKey("music.example.com/path/")).toBe(expected);
    expect(urlKey("example.com/path?q=2")).toBe(expected);
  });

  it("keys youtube watch, music.youtube watch, youtu.be and shorts urls by video id", () => {
    expect(urlKey("https://www.youtube.com/watch?v=AbC123")).toBe("yt:AbC123");
    expect(urlKey("https://music.youtube.com/watch?v=AbC123")).toBe("yt:AbC123");
    expect(urlKey("https://youtu.be/AbC123")).toBe("yt:AbC123");
    expect(urlKey("https://www.youtube.com/shorts/AbC123")).toBe("yt:AbC123");
  });

  it("keeps the youtube id case-sensitive", () => {
    expect(urlKey("https://youtu.be/AbC123")).not.toBe(urlKey("https://youtu.be/abc123"));
  });

  it("gives different keys for different youtube ids", () => {
    expect(urlKey("https://youtu.be/AAAAAAAAAAA")).not.toBe(urlKey("https://youtu.be/BBBBBBBBBBB"));
  });

  it("gives different keys for two different bandcamp tracks", () => {
    expect(urlKey("https://artist.bandcamp.com/track/one")).not.toBe(
      urlKey("https://artist.bandcamp.com/track/two"),
    );
  });

  it("sameUrl is true for equivalent urls", () => {
    expect(sameUrl("https://www.example.com/Path/?a=1#x", "example.com/path")).toBe(true);
    expect(sameUrl("https://youtu.be/AbC123", "https://www.youtube.com/watch?v=AbC123")).toBe(true);
  });

  it("sameUrl is false for different urls", () => {
    expect(sameUrl("https://artist.bandcamp.com/track/one", "https://artist.bandcamp.com/track/two")).toBe(false);
  });

  it("sameUrl is false when either side is empty", () => {
    expect(sameUrl("", "https://example.com/path")).toBe(false);
    expect(sameUrl("https://example.com/path", null)).toBe(false);
    expect(sameUrl(undefined, undefined)).toBe(false);
  });
});
