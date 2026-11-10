import { describe, expect, it, vi } from "vitest";
import type { RemoteRef } from "@/shared/lib/libraryIndex.ts";
import { pickStatuses, readStatus, refsKey, remoteRefOf } from "./useDownloadStatus.ts";

// The hooks module imports the store, which reads the library store at load time; the pure helpers need neither.
vi.mock("@/shared/store/libraryIndexStore.ts", () => ({ useLibraryIndexStore: () => undefined }));

const ref = (url: string): RemoteRef => ({ platform: "bandcamp", url });

describe("remoteRefOf", () => {
  it("takes the platform and the page url of a search item", () => {
    expect(
      remoteRefOf({ platform: "soundcloud", item_url_path: "https://soundcloud.com/a/b" }),
    ).toEqual({ platform: "soundcloud", url: "https://soundcloud.com/a/b" });
  });

  it("takes the album name of a track as its album", () => {
    expect(
      remoteRefOf({
        platform: "bandcamp",
        item_url_path: "https://a.test/track/1",
        name: "Song",
        band_name: "Band",
        album_name: "LP",
      }),
    ).toEqual({
      platform: "bandcamp",
      url: "https://a.test/track/1",
      artist: "Band",
      title: "Song",
      album: "LP",
    });
  });

  it("leaves the album undefined when the album name is null", () => {
    expect(
      remoteRefOf({ platform: "bandcamp", item_url_path: "https://a.test/track/1", album_name: null }).album,
    ).toBeUndefined();
  });
});

describe("refsKey", () => {
  it("is equal for the same urls in the same order and differs otherwise", () => {
    expect(refsKey([ref("a"), ref("b")])).toBe(refsKey([ref("a"), ref("b")]));
    expect(refsKey([ref("a"), ref("b")])).not.toBe(refsKey([ref("b"), ref("a")]));
    expect(refsKey([])).toBe("");
  });
});

describe("readStatus", () => {
  it("reads own keys only, so prototype names are not answers", () => {
    const cache = { a: "downloaded" as const };
    expect(readStatus(cache, "a")).toBe("downloaded");
    expect(readStatus(cache, "b")).toBeUndefined();
    expect(readStatus(cache, "constructor")).toBeUndefined();
  });
});

describe("pickStatuses", () => {
  it("returns only the requested urls that have an answer", () => {
    const cache = { a: "downloaded" as const, c: "partial" as const };
    expect(pickStatuses(cache, [ref("a"), ref("b"), ref("c")])).toEqual({
      a: "downloaded",
      c: "partial",
    });
  });

  it("is empty for no refs or an empty cache", () => {
    expect(pickStatuses({}, [ref("a")])).toEqual({});
    expect(pickStatuses({ a: "none" as const }, [])).toEqual({});
  });
});
