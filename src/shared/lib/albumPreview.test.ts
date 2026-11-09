import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ItemType } from "@/shared/store/libraryStore.ts";
import {
  albumInfoLine,
  downloadLabel,
  getAlbumPreview,
  selectableIndexes,
  selectedUrls,
  trackAsStreamItem,
  trackCountLabel,
  type AlbumPreview,
  type AlbumTrackInfo,
} from "./albumPreview.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@/shared/lib/search.ts", () => ({ searchPlatforms: vi.fn() }));

const track = (index: number, over: Partial<AlbumTrackInfo> = {}): AlbumTrackInfo => ({
  index,
  title: `Track ${index + 1}`,
  artists: [],
  track_number: index + 1,
  url: `https://example.test/track/${index}`,
  available: true,
  note: null,
  ...over,
});

const preview = (tracks: AlbumTrackInfo[], over: Partial<AlbumPreview> = {}): AlbumPreview => ({
  platform: "bandcamp",
  url: "https://example.test/album",
  title: "Album Title",
  artist: "Album Artist",
  cover_url: "https://example.test/cover.jpg",
  year: 2020,
  genres: [],
  tracks,
  ...over,
});

const tenTracks = (unavailable: number[] = []): AlbumTrackInfo[] =>
  Array.from({ length: 10 }, (_, i) =>
    unavailable.includes(i)
      ? track(i, { available: false, url: "", note: "Not available" })
      : track(i),
  );

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

describe("getAlbumPreview", () => {
  it("invokes the backend command with platform and url", async () => {
    const result = preview([track(0)]);
    vi.mocked(invoke).mockResolvedValueOnce(result);

    await expect(getAlbumPreview("bandcamp", "https://example.test/album")).resolves.toEqual(result);
    expect(invoke).toHaveBeenCalledWith("get_album_preview", {
      platform: "bandcamp",
      url: "https://example.test/album",
    });
  });
});

describe("selectableIndexes", () => {
  it("skips unavailable tracks and tracks without a url", () => {
    const p = preview([
      track(0),
      track(1, { available: false, url: "", note: "Gone" }),
      track(2, { url: "" }),
      track(3),
      track(4, { available: false }), // unavailable even with a url
    ]);
    expect(selectableIndexes(p)).toEqual([0, 3]);
  });

  it("returns an empty list when nothing can be downloaded", () => {
    expect(selectableIndexes(preview([track(0, { available: false })]))).toEqual([]);
    expect(selectableIndexes(preview([]))).toEqual([]);
  });
});

describe("selectedUrls", () => {
  it("returns the urls in album order, not selection order", () => {
    const p = preview([track(0), track(1), track(2), track(3), track(4)]);
    // Selected in the order 4, 0, 1 (a Set keeps insertion order).
    const selected = new Set([4, 0, 1]);
    expect(selectedUrls(p, selected)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/1",
      "https://example.test/track/4",
    ]);
  });

  it("drops unavailable tracks, tracks without a url and unknown indexes", () => {
    const p = preview([
      track(0),
      track(1),
      track(2, { available: false, url: "", note: "Gone" }),
      track(3, { url: "" }),
      track(4),
    ]);
    const selected = new Set([4, 3, 2, 0, 9, 1]);
    expect(selectedUrls(p, selected)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/1",
      "https://example.test/track/4",
    ]);
  });

  it("returns an empty list for an empty selection", () => {
    expect(selectedUrls(preview([track(0)]), new Set())).toEqual([]);
  });
});

describe("downloadLabel", () => {
  it.each([
    [0, 10, "Download album"],
    [10, 10, "Download album"],
    [3, 10, "Download 3 of 10 tracks"],
    [1, 10, "Download 1 track"],
    [0, 1, "Download track"],
    [1, 1, "Download track"],
  ])("selected %i of %i -> %s", (selected, available, expected) => {
    expect(downloadLabel(selected, available)).toBe(expected);
  });
});

describe("trackCountLabel", () => {
  it("counts a single track", () => {
    expect(trackCountLabel(preview([track(0)]))).toBe("1 track");
  });

  it("counts several tracks", () => {
    expect(trackCountLabel(preview(tenTracks()))).toBe("10 tracks");
  });

  it("appends the number of unavailable tracks", () => {
    expect(trackCountLabel(preview(tenTracks([3, 7])))).toBe("10 tracks · 2 unavailable");
  });
});

describe("albumInfoLine", () => {
  it("joins year and genres with a middle dot", () => {
    expect(albumInfoLine({ year: 2020, genres: ["Ambient", "Drone"] })).toBe(
      "2020 · Ambient, Drone",
    );
  });

  it("shows only the year when there are no genres", () => {
    expect(albumInfoLine({ year: 2020, genres: [] })).toBe("2020");
  });

  it("shows only the genres when there is no year", () => {
    expect(albumInfoLine({ year: null, genres: ["Ambient", "Drone"] })).toBe("Ambient, Drone");
  });

  it("is empty when there is neither a year nor genres", () => {
    expect(albumInfoLine({ year: null, genres: [] })).toBe("");
  });
});

describe("trackAsStreamItem", () => {
  const base = {
    platform: "youtube" as const,
    title: "Album Title",
    artist: "Album Artist",
    cover_url: "https://example.test/cover.jpg",
  };

  it("maps the track to a track search item", () => {
    const t = track(0, { title: "Song", artists: ["Featured Artist", "Other"] });
    expect(trackAsStreamItem(base, t)).toEqual({
      item_type: ItemType.Track,
      name: "Song",
      band_name: "Featured Artist",
      album_name: "Album Title",
      item_url_path: "https://example.test/track/0",
      img: "https://example.test/cover.jpg",
      platform: "youtube",
    });
  });

  it("falls back to the album artist when the track has no artists", () => {
    expect(trackAsStreamItem(base, track(0, { artists: [] })).band_name).toBe("Album Artist");
  });

  it("uses an empty image when the album has no cover", () => {
    expect(trackAsStreamItem({ ...base, cover_url: null }, track(0)).img).toBe("");
  });
});
