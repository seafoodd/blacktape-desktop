import { describe, it, expect, vi } from "vitest";
import {
  downloadingAlbumMetaLine,
  isActiveLibraryDownload,
  normalizeAlbumTitle,
  matchDownloadToLibraryAlbum,
  prepareAlbumDeletion,
  groupMissingSelection,
  extraDownloadingTracksForAlbum,
  unifiedAlbumKey,
  findMissingTrackDownload,
} from "./artistAlbumsDownloads.ts";
import { ItemType, type DownloadProgress } from "@/shared/store/libraryStore.ts";
import type { Song } from "@/shared/types.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";

it("groups only selected, available missing tracks by album without mixing their positions", () => {
  const first = { album: "In Utero", track: { index: 11, url: "missing-one" } };
  const second = { album: "Nevermind", track: { index: 11, url: "missing-two" } };
  const pending = { album: "Nevermind", track: { index: 12, url: "pending" } };
  expect([...groupMissingSelection([first, second, pending], (url) => url !== "pending")])
    .toEqual([["In Utero", [first]], ["Nevermind", [second]]]);
});

function dl(over: Partial<DownloadProgress> = {}): DownloadProgress {
  return {
    id: "dl-1",
    url: "https://example.test/album/morrison-hotel",
    platform: "bandcamp",
    itemType: ItemType.Album,
    artistName: "The Doors",
    albumTitle: "Morrison Hotel",
    overallPercent: 20,
    current: 2,
    total: 11,
    message: "Downloading...",
    status: "downloading",
    tracks: [
      { title: "Roadhouse Blues", status: "downloading", percent: 50, trackNumber: 1 },
      { title: "Waiting for the Sun", status: "pending", percent: 0, trackNumber: 2 },
    ],
    ...over,
  };
}

function missingTrack(overrides: Partial<AlbumTrackInfo> = {}): AlbumTrackInfo {
  return {
    index: 0,
    title: "Song",
    artists: [],
    track_number: 1,
    disc_number: 1,
    url: "",
    available: true,
    note: null,
    ...overrides,
  };
}

describe("artistAlbumsDownloads", () => {
  it("drains matching downloads before reading the songs to delete", async () => {
    const album = { title: "Morrison Hotel", songs: [{ id: 1 }] };
    let finish!: () => void;
    const drain = new Promise<void>((resolve) => { finish = resolve; });
    const cancel = vi.fn(() => drain);
    const saved = { id: 2 } as Song;
    const fetchAlbums = vi.fn(async () => [{
      title: album.title, album_artist: "The Doors", cover_url: null, songs: [saved],
    }]);
    const deletion = prepareAlbumDeletion(album, "The Doors", [
      dl({ id: "linked", status: "idle", albumTitle: "Original title" }),
      dl({ id: "upgrade", url: "resolve:song:1", albumTitle: "" }),
      dl({ id: "other", url: "https://example.test/other", artistName: "Different artist" }),
      dl({ id: "completed", status: "completed" }),
    ], dl().url, cancel, fetchAlbums);
    expect(cancel.mock.calls).toEqual([["linked"], ["upgrade"]]);
    expect(fetchAlbums).not.toHaveBeenCalled();
    finish();
    await expect(deletion).resolves.toEqual([saved]);
  });

  it("detects active downloads and ignores completed/failed/cancelled or resolve URLs", () => {
    expect(isActiveLibraryDownload(dl({ status: "downloading" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "idle" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "processing" }))).toBe(true);
    expect(isActiveLibraryDownload(dl({ status: "downloading", url: "resolve:abc" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "completed" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "failed" }))).toBe(false);
    expect(isActiveLibraryDownload(dl({ status: "cancelled" }))).toBe(false);
  });

  it("normalizes album titles by trimming and lowercasing", () => {
    expect(normalizeAlbumTitle("  Morrison Hotel  ")).toBe("morrison hotel");
    expect(normalizeAlbumTitle("THE MONEY STORE")).toBe("the money store");
  });

  it("matches downloads to existing library albums by linked source URL or normalized album title", () => {
    const albums = [{ title: "Morrison Hotel" }, { title: "L.A. Woman" }];
    const missingSources = {
      "Morrison Hotel": {
        album: "Morrison Hotel",
        platform: "bandcamp" as const,
        source_url: "https://doors.bandcamp.com/album/morrison-hotel",
        cover_url: "",
        total: 11,
        owned: 0,
        missing: 11,
      },
    };

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://doors.bandcamp.com/album/morrison-hotel", albumTitle: "Other Title" }),
        albums,
        missingSources,
        true,
      ),
    ).toBe("Morrison Hotel");

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://youtube.com/playlist?list=123", albumTitle: "morrison hotel" }),
        albums,
        missingSources,
        true,
      ),
    ).toBe("Morrison Hotel");

    expect(
      matchDownloadToLibraryAlbum(
        dl({ url: "https://youtube.com/playlist?list=999", albumTitle: "Strange Days" }),
        albums,
        missingSources,
        true,
      ),
    ).toBeUndefined();
  });

  it("filters out downloading tracks that already exist in the library album or missing tracks", () => {
    const librarySongs = [{ title: "Roadhouse Blues" }];
    const extra = extraDownloadingTracksForAlbum(librarySongs, ["Peace Frog"], [dl()]);
    expect(extra.map((t) => t.title)).toEqual(["Waiting for the Sun"]);
  });

  it("does not match a different title that happens to share the missing row's track number", () => {
    expect(findMissingTrackDownload([
      dl({ tracks: [{ title: "Different Song", status: "downloading", percent: 30, trackNumber: 1 }] }),
    ], missingTrack())).toBeUndefined();
  });

  it("uses known disc metadata to distinguish same-titled tracks across discs", () => {
    const discOne = dl({
      id: "disc-one",
      tracks: [{ title: "Song", status: "downloading", percent: 20, trackNumber: 1, discNumber: 1 }],
    });
    const discTwo = dl({
      id: "disc-two",
      tracks: [{ title: "Song", status: "downloading", percent: 40, trackNumber: 1, discNumber: 2 }],
    });

    expect(findMissingTrackDownload([discOne, discTwo], missingTrack({ disc_number: 2 }))?.id).toBe("disc-two");
  });

  it("does not choose between equally strong recovered matches when the row has no disc", () => {
    const recoveredOne = dl({
      id: "recovered-one",
      tracks: [{ title: "Song", status: "downloading", percent: 20, trackNumber: 1 }],
    });
    const recoveredTwo = dl({
      id: "recovered-two",
      tracks: [{ title: "Song", status: "downloading", percent: 40, trackNumber: 1 }],
    });

    expect(findMissingTrackDownload([recoveredOne, recoveredTwo], missingTrack({ disc_number: null }))).toBeUndefined();
  });

  it("prefers an exact selected track URL over weaker title metadata and ignores completed tasks", () => {
    const exactUrl = dl({ id: "url-match", trackUrls: ["https://example.test/track/song"] });
    const titleMatch = dl({
      id: "title-match",
      tracks: [{ title: "Song", status: "downloading", percent: 30, trackNumber: 1 }],
    });
    const completedUrl = dl({
      id: "completed",
      status: "completed",
      trackUrls: ["https://example.test/track/song"],
    });

    expect(findMissingTrackDownload([titleMatch, completedUrl, exactUrl], missingTrack({
      url: "https://example.test/track/song",
    }))?.id).toBe("url-match");
  });

  it("matches a unique placement task by normalized title and uses its library disc", () => {
    const placement = dl({
      id: "placement",
      itemType: ItemType.Track,
      tracks: [],
      placement: {
        title: "  SONG ",
        track_number: 1,
        album: "Morrison Hotel",
        album_artist: "The Doors",
        disc_number: 2,
      },
    });

    expect(findMissingTrackDownload([placement], missingTrack({ title: "Song", disc_number: 2 }))?.id)
      .toBe("placement");
  });

  it.each(["completed", "failed"] as const)(
    "does not offer cancellation for a %s track while a sibling in its album task is active",
    (trackStatus) => {
      const task = dl({
        tracks: [
          { title: "Song", status: trackStatus, percent: 100, trackNumber: 1, discNumber: 1 },
          { title: "Sibling", status: "downloading", percent: 25, trackNumber: 2, discNumber: 1 },
        ],
      });

      expect(findMissingTrackDownload([task], missingTrack())).toBeUndefined();
    },
  );

  it.each(["completed", "failed"] as const)(
    "does not use an exact selected track URL to offer cancellation for a %s track",
    (trackStatus) => {
      const task = dl({
        trackUrls: ["https://example.test/track/song"],
        tracks: [
          { title: "Song", status: trackStatus, percent: 100, trackNumber: 1, discNumber: 1 },
          { title: "Sibling", status: "downloading", percent: 25, trackNumber: 2, discNumber: 1 },
        ],
      });

      expect(findMissingTrackDownload([task], missingTrack({
        url: "https://example.test/track/song",
      }))).toBeUndefined();
    },
  );

  it("continues matching a track at 100 percent while its progress status is downloading", () => {
    const task = dl({
      tracks: [{ title: "Song", status: "downloading", percent: 100, trackNumber: 1, discNumber: 1 }],
    });

    expect(findMissingTrackDownload([task], missingTrack())?.id).toBe("dl-1");
  });

  it("rejects same-title same-disc progress when both known track numbers differ", () => {
    const task = dl({
      tracks: [{ title: "Intro", status: "downloading", percent: 50, trackNumber: 3, discNumber: 1 }],
    });

    expect(findMissingTrackDownload([task], missingTrack({
      title: "Intro",
      track_number: 1,
      disc_number: 1,
    }))).toBeUndefined();
  });

  it("rejects placement progress when both known track numbers differ", () => {
    const task = dl({
      itemType: ItemType.Track,
      tracks: [],
      placement: {
        title: "Intro",
        track_number: 3,
        album: "Morrison Hotel",
        album_artist: "The Doors",
        disc_number: 1,
      },
    });

    expect(findMissingTrackDownload([task], missingTrack({
      title: "Intro",
      track_number: 1,
      disc_number: 1,
    }))).toBeUndefined();
  });

  it("preserves each originating task id and placement fallback while filtering represented tracks", () => {
    const first = dl({
      id: "album-task",
      tracks: [
        { title: "Saved Song", status: "downloading", percent: 15, trackNumber: 1 },
        { title: "Missing Song", status: "pending", percent: 0, trackNumber: 2 },
        { title: "New Song", status: "downloading", percent: 40, trackNumber: 3 },
      ],
    });
    const second = dl({
      id: "placement-task",
      itemType: ItemType.Track,
      tracks: [],
      overallPercent: 65,
      placement: {
        title: "Placement Song",
        track_number: 4,
        album: "Morrison Hotel",
        album_artist: "The Doors",
      },
    });

    expect(extraDownloadingTracksForAlbum([{ title: "Saved Song" }], ["Missing Song"], [first, second]))
      .toMatchObject([
        {
          taskId: "album-task", trackIndex: 2, title: "New Song", trackNumber: 3, discNumber: 1,
          percent: 40, status: "downloading",
        },
        {
          taskId: "placement-task", trackIndex: 0, title: "Placement Song", trackNumber: 4, discNumber: 1,
          percent: 65, status: "downloading",
        },
      ]);
  });

  it("produces identical unifiedAlbumKey across downloading and library albums and formats release year meta", () => {
    expect(unifiedAlbumKey("Aphex Twin", " Drukqs ")).toBe(
      unifiedAlbumKey("aphex twin", "drukqs"),
    );
    expect(downloadingAlbumMetaLine({ releaseYear: 2001 })).toBe("2001");
    expect(downloadingAlbumMetaLine({ releaseYear: undefined })).toBeNull();
  });
});
