import { describe, it, expect, vi } from "vitest";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { LibraryAlbumSource, MissingTracks } from "@/shared/lib/libraryIndex.ts";
import {
  missingAlbumMenuItems,
  missingRemoteTrack,
  missingRemoteTracks,
  missingStreamItems,
  missingTrackNumber,
} from "./missingTrackRow.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/libraryIndex.ts", () => ({
  getArtistAlbumSources: vi.fn(),
  getMissingTracks: vi.fn(),
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: { getState: () => ({ startDownload: vi.fn() }), subscribe: vi.fn() },
}));

const track = (index: number, over: Partial<AlbumTrackInfo> = {}): AlbumTrackInfo => ({
  index,
  title: `Track ${index + 1}`,
  artists: ["Guest"],
  track_number: index + 1,
  url: `https://example.test/track/${index}`,
  available: true,
  note: null,
  duration_ms: 185000,
  ...over,
});

const missing = (over: Partial<MissingTracks> = {}): MissingTracks => ({
  platform: "bandcamp",
  source_url: "https://example.test/album/A",
  album_title: "A",
  album_artist: "Artist",
  cover_url: "https://example.test/cover.jpg",
  tracks: [track(1)],
  ...over,
});

const source = (over: Partial<LibraryAlbumSource> = {}): LibraryAlbumSource => ({
  album: "A",
  platform: "bandcamp",
  source_url: "https://example.test/album/A",
  cover_url: null,
  total: 4,
  owned: 2,
  missing: 2,
  ...over,
});

const actions = () => ({ toggle: vi.fn(), download: vi.fn() });

describe("missingRemoteTracks", () => {
  it("maps the playable missing songs to remote tracks of the source album", () => {
    const list = missingRemoteTracks(
      missing({ tracks: [track(0), track(1, { available: false, url: "" }), track(2)] }),
    );
    expect(list.map((t) => t.url)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/2",
    ]);
    expect(list[0]).toEqual({
      platform: "bandcamp",
      url: "https://example.test/track/0",
      title: "Track 1",
      artist: "Guest",
      album: "A",
      album_url: "https://example.test/album/A",
      cover: "https://example.test/cover.jpg",
      duration_ms: 185000,
    });
  });

  it("falls back to the album artist when the track names no artist", () => {
    const [remote] = missingRemoteTracks(missing({ tracks: [track(0, { artists: [] })] }));
    expect(remote.artist).toBe("Artist");
  });

  it("is empty when nothing is missing", () => {
    expect(missingRemoteTracks(missing({ tracks: [] }))).toEqual([]);
  });
});

describe("missingRemoteTrack", () => {
  it("gives the remote track of one song, with a null duration when unknown", () => {
    const remote = missingRemoteTrack(missing(), track(1, { duration_ms: null }));
    expect(remote.url).toBe("https://example.test/track/1");
    expect(remote.album_url).toBe("https://example.test/album/A");
    expect(remote.duration_ms).toBeNull();
  });

  it("uses a null cover when the album has none", () => {
    const remote = missingRemoteTrack(missing({ cover_url: null }), track(1));
    expect(remote.cover).toBeNull();
  });
});

describe("missingStreamItems", () => {
  it("gives search items in the same order, with the duration in seconds", () => {
    const items = missingStreamItems(missing({ tracks: [track(0), track(1)] }));
    expect(items.map((i) => i.item_url_path)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/1",
    ]);
    expect(items[0].duration).toBe(185);
    expect(items[0].platform).toBe("bandcamp");
    expect(items[0].band_name).toBe("Guest");
  });
});

describe("missingTrackNumber", () => {
  it("uses the track number, padded to two digits", () => {
    expect(missingTrackNumber({ track_number: 7, index: 0 })).toBe("07");
  });

  it("falls back to the position in the album when the number is unknown", () => {
    expect(missingTrackNumber({ track_number: null, index: 11 })).toBe("12");
  });
});

describe("missingAlbumMenuItems", () => {
  it("has no entries for an album without a source", () => {
    expect(missingAlbumMenuItems(undefined, false, actions())).toEqual([]);
  });

  it("offers show and download when songs are missing and none is shown", () => {
    const a = actions();
    const items = missingAlbumMenuItems(source({ missing: 2 }), false, a);
    expect(items.map((i) => i.label)).toEqual([
      "",
      "Show missing songs (2)",
      "Download missing (2)",
    ]);
    expect(items[0].separator).toBe(true);
    items[1].action?.();
    items[2].action?.();
    expect(a.toggle).toHaveBeenCalledTimes(1);
    expect(a.download).toHaveBeenCalledTimes(1);
  });

  it("offers hide, and download while songs are missing, when the missing songs are shown", () => {
    const items = missingAlbumMenuItems(source({ missing: 1 }), true, actions());
    expect(items.map((i) => i.label)).toEqual(["", "Hide missing songs", "Download missing (1)"]);
  });

  it("offers nothing when no song is missing", () => {
    expect(missingAlbumMenuItems(source({ missing: 0 }), false, actions())).toEqual([]);
  });

  it("still offers hide when the shown songs are all in the library now", () => {
    const items = missingAlbumMenuItems(source({ missing: 0 }), true, actions());
    expect(items.map((i) => i.label)).toEqual(["", "Hide missing songs"]);
  });
});
