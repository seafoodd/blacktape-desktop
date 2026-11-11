import { afterEach, describe, expect, it, vi } from "vitest";
import { coverSrc, sizedRemoteCover } from "./cover.ts";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string, proto = "asset") => `${proto}://${p}`,
}));

const bandcamp = "https://f4.bcbits.com/img/a3251624428_0.jpg";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("sizedRemoteCover", () => {
  it.each([
    [40, "_42.jpg"],
    [50, "_42.jpg"],
    [64, "_3.jpg"],
    [100, "_3.jpg"],
    [128, "_7.jpg"],
    [200, "_44.jpg"],
    [350, "_2.jpg"],
    [400, "_16.jpg"],
    [2000, "_10.jpg"],
  ])("Bandcamp album art at %i px picks format %s", (px, suffix) => {
    expect(sizedRemoteCover(bandcamp, px)).toBe(`https://f4.bcbits.com/img/a3251624428${suffix}`);
  });

  it("rewrites a png or other format id the same way and keeps a query string", () => {
    expect(sizedRemoteCover("https://f4.bcbits.com/img/a3251624428_10.jpg", 64)).toBe(
      "https://f4.bcbits.com/img/a3251624428_3.jpg",
    );
    expect(sizedRemoteCover("https://f4.bcbits.com/img/a3251624428_16.png", 400)).toBe(
      "https://f4.bcbits.com/img/a3251624428_16.jpg",
    );
    expect(sizedRemoteCover("https://f4.bcbits.com/img/a3251624428_16.jpg?v=2", 128)).toBe(
      "https://f4.bcbits.com/img/a3251624428_7.jpg?v=2",
    );
  });

  it("leaves non-album Bandcamp images unchanged", () => {
    const url = "https://f4.bcbits.com/img/0012345678_10.jpg";
    expect(sizedRemoteCover(url, 64)).toBe(url);
  });

  it("rewrites Google image sizes for yt3 and lh3 hosts", () => {
    expect(sizedRemoteCover("https://yt3.googleusercontent.com/abc=w544-h544-l90-rj", 64)).toBe(
      "https://yt3.googleusercontent.com/abc=w64-h64-l90-rj",
    );
    expect(sizedRemoteCover("https://lh3.googleusercontent.com/abc=w544-h544-l90-rj", 64)).toBe(
      "https://lh3.googleusercontent.com/abc=w64-h64-l90-rj",
    );
  });

  it("rounds a fractional Google px up", () => {
    expect(sizedRemoteCover("https://yt3.googleusercontent.com/abc=w544-h544-l90-rj", 63.2)).toBe(
      "https://yt3.googleusercontent.com/abc=w64-h64-l90-rj",
    );
  });

  it("leaves a Google url without a size suffix unchanged", () => {
    const url = "https://yt3.googleusercontent.com/abc-l90-rj";
    expect(sizedRemoteCover(url, 64)).toBe(url);
  });

  it.each([
    "https://i.ytimg.com/vi/x/hqdefault.jpg",
    "https://example.com/a_0.jpg",
  ])("leaves other url %s unchanged", (url) => {
    expect(sizedRemoteCover(url, 64)).toBe(url);
  });
});

describe("coverSrc", () => {
  it("returns a remote url unchanged when no cssPx is given", () => {
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    expect(coverSrc(bandcamp)).toBe(bandcamp);
  });

  it("sizes a remote url by devicePixelRatio when cssPx is given", () => {
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    // 64 css px * 2 = 128 device px -> smallest side >= 128 is 150 -> format 7
    expect(coverSrc(bandcamp, 64)).toBe("https://f4.bcbits.com/img/a3251624428_7.jpg");
  });

  it("treats a missing devicePixelRatio as 1", () => {
    vi.stubGlobal("window", {});
    expect(coverSrc(bandcamp, 64)).toBe("https://f4.bcbits.com/img/a3251624428_3.jpg");
  });

  it("returns the asset url for a local path without cssPx", () => {
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    expect(coverSrc("C:/music/cover.jpg")).toBe("asset://C:/music/cover.jpg");
  });

  it("returns a thumb url with the device-pixel size for a local path", () => {
    vi.stubGlobal("window", { devicePixelRatio: 2 });
    expect(coverSrc("C:/music/cover.jpg", 64)).toBe("thumb://C:/music/cover.jpg?s=128");
  });
});
