import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { DownloadState } from "@/shared/lib/library/libraryIndex.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import {
  albumAction,
  albumInfoLine,
  albumLibraryState,
  albumMetadataSourceBadge,
  albumMetaLine,
  candidateMatchWarning,
  downloadLabel,
  estimatedPlatformQuality,
  formatEditionSubtext,
  getAlbumPreview,
  isTrackRowActive,
  partitionEditions,
  selectableIndexes,
  selectableTracks,
  selectedUrls,
  trackAsStreamItem,
  trackCountLabel,
  trackRefs,
  trackRowStatus,
  trackTaskStates,
  upgradableAlbumTracks,
  unverifiedTracksForDownload,
  type AlbumPreview,
  type AlbumTrackInfo,
} from "./albumPreview.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@/shared/lib/search/search.ts", () => ({ searchPlatforms: vi.fn() }));

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

describe("selectableTracks", () => {
  it("returns the same tracks as selectableIndexes, in album order", () => {
    const p = preview([track(0), track(1, { available: false, url: "" }), track(2), track(3, { url: "" })]);
    expect(selectableTracks(p).map((t) => t.index)).toEqual(selectableIndexes(p));
    expect(selectableTracks(p).map((t) => t.index)).toEqual([0, 2]);
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

  it("appends the total length after the year and genres", () => {
    expect(
      albumInfoLine({ year: 2020, genres: ["Ambient"], total_duration_ms: 3_900_000 }),
    ).toBe("2020 · Ambient · 1 hr 5 min");
  });

  it("shows only the total length when year and genres are missing", () => {
    expect(albumInfoLine({ year: null, genres: [], total_duration_ms: 1_800_000 })).toBe("30 min");
  });

  it("ignores a null or missing total length", () => {
    expect(albumInfoLine({ year: 2020, genres: [], total_duration_ms: null })).toBe("2020");
    expect(albumInfoLine({ year: 2020, genres: [], total_duration_ms: undefined })).toBe("2020");
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

  it("carries the track duration in whole seconds", () => {
    expect(trackAsStreamItem(base, track(0, { duration_ms: 185_400 })).duration).toBe(185);
  });

  it("leaves the duration out when the track has none", () => {
    expect(trackAsStreamItem(base, track(0, { duration_ms: null })).duration).toBeUndefined();
  });

  it("uses the track URL platform instead of preview.platform when playing Bandcamp or SoundCloud inside MusicBrainz albums", () => {
    const bcTrack = track(0, { url: "https://heavenpierceher.bandcamp.com/track/the-fall" });
    expect(trackAsStreamItem(base, bcTrack).platform).toBe("bandcamp");

    const scTrack = track(1, { url: "https://soundcloud.com/artist/track" });
    expect(trackAsStreamItem(base, scTrack).platform).toBe("soundcloud");
  });
});

describe("trackRefs", () => {
  it("asks the library about the selectable tracks only, with the album platform", () => {
    const p = preview([
      track(0),
      track(1, { available: false, url: "", note: "Gone" }),
      track(2, { url: "" }),
      track(3),
    ]);
    expect(trackRefs(p).map((r) => [r.platform, r.url])).toEqual([
      ["bandcamp", "https://example.test/track/0"],
      ["bandcamp", "https://example.test/track/3"],
    ]);
  });

  it("carries artist and title so songs without a stored url can be recognised", () => {
    const p = preview([track(0, { title: "Song", artists: ["Band"] })]);
    expect(trackRefs(p)[0]).toMatchObject({ title: "Song", artist: "Band" });
  });

  it("gives each ref the album title of the preview as its album", () => {
    const p = preview([track(0), track(1, { url: "https://example.test/track/1" })], { title: "Deep Black" });
    expect(trackRefs(p).map((r) => r.album)).toEqual(["Deep Black", "Deep Black"]);
  });
});

describe("albumLibraryState", () => {
  const url = (i: number) => `https://example.test/track/${i}`;

  it("is downloaded when every selectable track is in the library", () => {
    const p = preview([track(0), track(1), track(2, { available: false, url: "" }), track(3)]);
    expect(
      albumLibraryState(p, { [url(0)]: "downloaded", [url(1)]: "downloaded", [url(3)]: "downloaded" }),
    ).toBe("downloaded");
  });

  it("is partial when some selectable tracks are in the library", () => {
    const p = preview([track(0), track(1), track(2)]);
    expect(albumLibraryState(p, { [url(1)]: "downloaded" })).toBe("partial");
  });

  it("is none when no selectable track is in the library or the states are unknown", () => {
    const p = preview([track(0), track(1)]);
    expect(albumLibraryState(p, {})).toBe("none");
    expect(albumLibraryState(p, { [url(0)]: "none", [url(1)]: "none" })).toBe("none");
  });

  it("ignores unavailable tracks, even when their url has a state", () => {
    const p = preview([track(0), track(1, { available: false, url: url(1) })]);
    expect(albumLibraryState(p, { [url(0)]: "downloaded", [url(1)]: "none" })).toBe("downloaded");
  });

  it("is none for an album without selectable tracks", () => {
    expect(albumLibraryState(preview([track(0, { available: false })]), {})).toBe("none");
  });
});

describe("trackTaskStates", () => {
  const u = (i: number) => `https://example.test/track/${i}`;

  it("reports every url of a running task with the task percent", () => {
    expect(
      trackTaskStates(
        { "task-1": [u(0), u(1)] },
        { "task-1": { status: "downloading", overallPercent: 40 } },
      ),
    ).toEqual({
      [u(0)]: { phase: "running", taskId: "task-1", percent: 40 },
      [u(1)]: { phase: "running", taskId: "task-1", percent: 40 },
    });
  });

  it.each([
    ["idle", 0],
    ["processing", 90],
  ])("treats status %s as running", (status, percent) => {
    expect(
      trackTaskStates({ t: [u(0)] }, { t: { status, overallPercent: percent } })[u(0)],
    ).toEqual({ phase: "running", taskId: "t", percent });
  });

  it.each([
    [150, 100],
    [-3, 0],
    [Number.NaN, 0],
    [33.6, 34],
  ])("clamps the percent %s to %s", (raw, expected) => {
    expect(
      trackTaskStates({ t: [u(0)] }, { t: { status: "downloading", overallPercent: raw } })[u(0)],
    ).toEqual({ phase: "running", taskId: "t", percent: expected });
  });

  it("reports a completed task as done", () => {
    expect(
      trackTaskStates({ t: [u(0)] }, { t: { status: "completed", overallPercent: 100 } })[u(0)],
    ).toEqual({ phase: "done" });
  });

  it("omits failed and cancelled tasks, so the row shows its button again", () => {
    expect(trackTaskStates({ t: [u(0)] }, { t: { status: "failed", overallPercent: 10 } })).toEqual({});
    expect(
      trackTaskStates({ t: [u(0)] }, { t: { status: "cancelled", overallPercent: 10 } }),
    ).toEqual({});
  });

  it("omits tasks the library no longer knows", () => {
    expect(trackTaskStates({ gone: [u(0)] }, {})).toEqual({});
  });

  it("reports distinct per-track percentages when detailed task.tracks are present", () => {
    const previewTracks = [
      track(0, { title: "Track One", track_number: 1 }),
      track(1, { title: "Track Two", track_number: 2 }),
      track(2, { title: "Track Three", track_number: 3 }),
    ];
    const task = {
      status: "downloading",
      overallPercent: 50,
      tracks: [
        { title: "Track One", percent: 100, status: "completed", trackNumber: 1 },
        { title: "Track Two", percent: 45, status: "downloading", trackNumber: 2 },
        { title: "Track Three", percent: 0, status: "pending", trackNumber: 3 },
      ],
    };
    expect(
      trackTaskStates(
        { "task-1": [u(0), u(1), u(2)] },
        { "task-1": task },
        previewTracks,
      ),
    ).toEqual({
      [u(0)]: { phase: "running", taskId: "task-1", percent: 100 },
      [u(1)]: { phase: "running", taskId: "task-1", percent: 45 },
      [u(2)]: { phase: "running", taskId: "task-1", percent: 0 },
    });
  });

  it("discovers active album download by previewUrl when taskTracks is empty", () => {
    const previewTracks = [
      track(0, { title: "Track One", track_number: 1 }),
      track(1, { title: "Track Two", track_number: 2 }),
    ];
    const albumUrl = "https://example.test/album/1";
    const task = {
      url: albumUrl,
      status: "downloading",
      overallPercent: 50,
      tracks: [
        { title: "Track One", percent: 100, status: "completed" },
        { title: "Track Two", percent: 15, status: "downloading" },
      ],
    };
    expect(
      trackTaskStates({}, { "task-1": task }, previewTracks, albumUrl),
    ).toEqual({
      [u(0)]: { phase: "running", taskId: "task-1", percent: 100 },
      [u(1)]: { phase: "running", taskId: "task-1", percent: 15 },
    });
  });
});

describe("trackRowStatus", () => {
  const available = { available: true, url: "https://example.test/track/0" };
  const running = { phase: "running" as const, percent: 25 };

  it("is unavailable for an unavailable track or one without a url, whatever the state", () => {
    expect(trackRowStatus({ available: false, url: available.url }, "downloaded", undefined)).toEqual({
      kind: "unavailable",
    });
    expect(trackRowStatus({ available: true, url: "" }, undefined, undefined)).toEqual({
      kind: "unavailable",
    });
  });

  it("shows the badge when the library has the track", () => {
    expect(trackRowStatus(available, "downloaded", undefined)).toEqual({ kind: "downloaded" });
  });

  it("shows the badge when its task just completed", () => {
    expect(trackRowStatus(available, "none", { phase: "done" })).toEqual({ kind: "downloaded" });
  });

  it("shows the progress of a running task", () => {
    expect(trackRowStatus(available, "none", running)).toEqual({ kind: "running", percent: 25 });
  });

  it("prefers the library state over a running task", () => {
    expect(trackRowStatus(available, "downloaded", running)).toEqual({ kind: "downloaded" });
  });

  it.each([
    ["no state", undefined],
    ["none", "none" as const],
    ["partial", "partial" as const],
  ])("offers the download button for %s", (_label, state) => {
    expect(trackRowStatus(available, state, undefined)).toEqual({ kind: "download" });
  });
});

describe("albumAction", () => {
  const url = (i: number) => `https://example.test/track/${i}`;
  const all = Array.from({ length: 10 }, (_, i) => i);
  const stateOf = (state: DownloadState, indexes: number[]): Record<string, DownloadState> =>
    Object.fromEntries(indexes.map((i) => [url(i), state]));

  it("downloads the whole album when nothing is in the library", () => {
    expect(albumAction(preview(tenTracks()), {}, 0)).toEqual({
      kind: "download",
      label: "Download album",
    });
  });

  it("treats explicit none states like missing ones", () => {
    expect(albumAction(preview(tenTracks()), stateOf("none", all), 0)).toEqual({
      kind: "download",
      label: "Download album",
    });
  });

  it("says track instead of album for a single-track album", () => {
    expect(albumAction(preview([track(0)]), {}, 0)).toEqual({
      kind: "download",
      label: "Download track",
    });
  });

  it("goes to the album when every selectable track is downloaded", () => {
    const result = albumAction(preview(tenTracks()), stateOf("downloaded", all), 0);
    expect(result).toEqual({ kind: "go-to-album", label: "Go to album" });
    expect(result.urls).toBeUndefined();
  });

  it("ignores unavailable tracks when everything selectable is downloaded", () => {
    const states = stateOf("downloaded", [0, 1, 2, 4, 5, 6, 8, 9]);
    expect(albumAction(preview(tenTracks([3, 7])), states, 0)).toEqual({
      kind: "go-to-album",
      label: "Go to album",
    });
  });

  it("ignores an unavailable track whose url has a none state", () => {
    const p = preview([track(0), track(1, { available: false })]);
    expect(albumAction(p, { [url(0)]: "downloaded", [url(1)]: "none" }, 0)).toEqual({
      kind: "go-to-album",
      label: "Go to album",
    });
  });

  it("ignores an available track without a url", () => {
    const p = preview([track(0), track(1, { url: "" })]);
    expect(albumAction(p, { [url(0)]: "downloaded" }, 0)).toEqual({
      kind: "go-to-album",
      label: "Go to album",
    });
  });

  it("downloads only the missing tracks when some are downloaded", () => {
    const result = albumAction(preview(tenTracks()), stateOf("downloaded", [0, 1, 2, 3, 4]), 0);
    expect(result).toEqual({
      kind: "download-missing",
      label: "Download missing tracks",
      urls: [url(5), url(6), url(7), url(8), url(9)],
    });
  });

  it("lists the missing urls in album order, whatever the order of the states", () => {
    const result = albumAction(preview(tenTracks()), stateOf("downloaded", [7, 2, 5]), 0);
    expect(result).toMatchObject({
      kind: "download-missing",
      urls: [url(0), url(1), url(3), url(4), url(6), url(8), url(9)],
    });
  });

  it("counts a partial track as missing", () => {
    const states: Record<string, DownloadState> = {
      [url(0)]: "downloaded",
      [url(1)]: "partial",
    };
    const result = albumAction(preview(tenTracks()), states, 0);
    expect(result).toEqual({
      kind: "download-missing",
      label: "Download missing tracks",
      urls: [url(1), url(2), url(3), url(4), url(5), url(6), url(7), url(8), url(9)],
    });
  });

  it("uses the singular label when exactly one track is missing", () => {
    expect(albumAction(preview(tenTracks()), stateOf("downloaded", [0, 1, 2, 3, 4, 5, 6, 7, 8]), 0)).toEqual({
      kind: "download-missing",
      label: "Download missing track",
      urls: [url(9)],
    });
  });

  it("does not count unavailable tracks as missing", () => {
    const states = stateOf("downloaded", [0, 1, 2, 4, 5, 6, 7, 8]);
    expect(albumAction(preview(tenTracks([3])), states, 0)).toEqual({
      kind: "download-missing",
      label: "Download missing track",
      urls: [url(9)],
    });
  });

  it.each([
    [1, "Download 1 track"],
    [3, "Download 3 of 10 tracks"],
    [10, "Download album"],
  ])("with a selection of %i downloads it even when everything is downloaded", (selectedCount, label) => {
    const result = albumAction(preview(tenTracks()), stateOf("downloaded", all), selectedCount);
    expect(result).toEqual({ kind: "download", label });
    expect(result.urls).toBeUndefined();
  });

  it("downloads the selection, not the missing tracks, when some are downloaded", () => {
    const result = albumAction(preview(tenTracks()), stateOf("downloaded", [0, 1]), 2);
    expect(result).toEqual({ kind: "download", label: "Download 2 of 10 tracks" });
  });

  it("labels a selection in a single-track album as the track", () => {
    expect(albumAction(preview([track(0)]), {}, 1)).toEqual({
      kind: "download",
      label: "Download track",
    });
  });

  it("gives a download action for an album without selectable tracks", () => {
    const onlyUnavailable = preview([track(0, { available: false, url: "", note: "Gone" })]);
    expect(albumAction(onlyUnavailable, {}, 0)).toEqual({
      kind: "download",
      label: "Download album",
    });
    expect(albumAction(preview([]), {}, 0)).toEqual({ kind: "download", label: "Download album" });
  });
});

describe("albumMetaLine", () => {
  it("joins the info line and the track count with a middle dot", () => {
    expect(
      albumMetaLine(
        preview(tenTracks(), { year: 2020, genres: ["Ambient"], total_duration_ms: 3_900_000 }),
      ),
    ).toBe("2020 · Ambient · 1 hr 5 min · 10 tracks");
  });

  it("shows only the track count when there is no year, genre or duration", () => {
    expect(albumMetaLine(preview([track(0), track(1), track(2)], { year: null, genres: [] }))).toBe(
      "3 tracks",
    );
  });

  it("uses the singular for a single track", () => {
    expect(albumMetaLine(preview([track(0)], { year: null, genres: [] }))).toBe("1 track");
  });

  it("appends the unavailable count at the end", () => {
    const p = preview(
      [track(0), track(1, { available: false, url: "", note: "Gone" }), track(2)],
      { year: null, genres: [] },
    );
    expect(albumMetaLine(p)).toBe("3 tracks · 1 unavailable");
  });

  it("puts the unavailable count after the info line", () => {
    const p = preview(
      [track(0), track(1, { available: false, url: "", note: "Gone" }), track(2)],
      { year: 2020, genres: [], total_duration_ms: null },
    );
    expect(albumMetaLine(p)).toBe("2020 · 3 tracks · 1 unavailable");
  });
});

describe("albumMetadataSourceBadge and estimatedPlatformQuality", () => {
  it("maps MusicBrainz and platform metadata sources", () => {
    expect(albumMetadataSourceBadge({ platform: "youtube", metadata_source: "musicbrainz", url: "canonical:rg:1" }))
      .toEqual({ label: "MusicBrainz", kind: "musicbrainz" });
    expect(albumMetadataSourceBadge({ platform: "bandcamp", metadata_source: "bandcamp", url: "https://a.bandcamp.com/album/x" }))
      .toEqual({ label: "Bandcamp", kind: "bandcamp" });
    expect(albumMetadataSourceBadge({ platform: "soundcloud", metadata_source: "soundcloud", url: "https://soundcloud.com/a/sets/b" }))
      .toEqual({ label: "SoundCloud", kind: "soundcloud" });
  });

  it("estimates lossless only for free Bandcamp albums and Opus High for YouTube", () => {
    expect(estimatedPlatformQuality({ platform: "youtube" }, "https://youtube.com/watch?v=1"))
      .toEqual({ label: "~ YT · OPUS (High)", tier: "high" });
    expect(estimatedPlatformQuality({ platform: "bandcamp", is_free_download: true }, "https://a.bandcamp.com/track/1"))
      .toEqual({ label: "~ BC · Lossless", tier: "lossless" });
    expect(estimatedPlatformQuality({ platform: "bandcamp", is_free_download: false }, "https://a.bandcamp.com/track/1"))
      .toEqual({ label: "~ BC · MP3 128k", tier: "standard" });
  });
});

describe("formatEditionSubtext and isTrackRowActive", () => {
  it("formats edition year, track count, and media format cleanly", () => {
    expect(
      formatEditionSubtext({ year: 2001, track_count: 30, format: "Digital Media" }),
    ).toBe("2001 · 30 tracks · Digital Media");
    expect(
      formatEditionSubtext({ year: null, track_count: 1, format: null }),
    ).toBe("1 track");
  });

  it("matches only local ID or active track URL, never unselected candidates", () => {
    const culDeSac = {
      available: true,
      title: "Cul-de-sac",
      url: "https://youtube.com/watch?v=culdesac",
    };
    // Playing Track 1 (Coelacanth) must NOT highlight Track 6 even if Coelacanth appeared as an unselected candidate elsewhere.
    expect(
      isTrackRowActive(culDeSac, undefined, { id: -1, title: "Coelacanth", album: "Plastic Death", source_url: "https://youtube.com/watch?v=coelacanth" }, "Plastic Death"),
    ).toBe(false);
    expect(
      isTrackRowActive(culDeSac, undefined, { id: -1, source_url: "https://youtube.com/watch?v=culdesac" }),
    ).toBe(true);
    expect(
      isTrackRowActive(culDeSac, 42, { id: 42, source_url: null }),
    ).toBe(true);
  });

  it("preserves highlight across release switches via album+track title when candidate matches or URL is still resolving", () => {
    const resolvingTrack = {
      available: true,
      title: "Jynweythek",
      url: "",
      candidates: [],
    };
    expect(
      isTrackRowActive(
        resolvingTrack,
        undefined,
        { id: -1, title: "Jynweythek", album: "Drukqs", source_url: "https://youtube.com/watch?v=old-release" },
        "Drukqs",
      ),
    ).toBe(true);

    const switchedTrackWithCandidate = {
      available: true,
      title: "Jynweythek",
      url: "https://bandcamp.com/track/jynweythek",
      candidates: [
        { source: { kind: "http_stream" }, url: "https://bandcamp.com/track/jynweythek" },
        { source: { kind: "http_stream" }, url: "https://youtube.com/watch?v=old-release" },
      ] as any,
    };
    expect(
      isTrackRowActive(
        switchedTrackWithCandidate,
        undefined,
        { id: -1, title: "Jynweythek", album: "Drukqs", source_url: "https://youtube.com/watch?v=old-release" },
        "Drukqs",
      ),
    ).toBe(true);

    // Different album with same track name should NOT highlight when URL differs
    expect(
      isTrackRowActive(
        resolvingTrack,
        undefined,
        { id: -1, title: "Jynweythek", album: "Other Album", source_url: "https://youtube.com/watch?v=other" },
        "Drukqs",
      ),
    ).toBe(false);
  });

  it("partitions MusicBrainz alternate editions and reveals all when expanded or selected", () => {
    const editions = [
      { id: "orig", title: "The Doors", edition_label: "Original edition", year: 1967, track_count: 11, disc_count: 1, format: "CD", is_default: true, is_alternate: false, source_kind: "musicbrainz" },
      { id: "mono", title: "The Doors", edition_label: "Mono", year: 1967, track_count: 11, disc_count: 1, format: '12" Vinyl', is_default: false, is_alternate: true, source_kind: "musicbrainz" },
      { id: "4cd", title: "The Doors", edition_label: "50th Anniversary", year: 2017, track_count: 41, disc_count: 4, format: "4×CD", is_default: false, is_alternate: false, source_kind: "musicbrainz" },
      { id: "yt", title: "The Doors", edition_label: "YouTube Music", year: 1967, track_count: 0, disc_count: 1, format: "Opus High", is_default: false, is_alternate: false, source_kind: "youtube" },
    ];

    const collapsed = partitionEditions(editions, "orig", false);
    expect(collapsed.visibleMb.map((e) => e.id)).toEqual(["orig", "4cd"]);
    expect(collapsed.hiddenMbCount).toBe(1);
    expect(collapsed.platformEditions.map((e) => e.id)).toEqual(["yt"]);

    const selectedAlt = partitionEditions(editions, "mono", false);
    expect(selectedAlt.visibleMb.map((e) => e.id)).toEqual(["orig", "mono", "4cd"]);
    expect(selectedAlt.hiddenMbCount).toBe(0);

    const expanded = partitionEditions(editions, "orig", true);
    expect(expanded.visibleMb.map((e) => e.id)).toEqual(["orig", "mono", "4cd"]);
    expect(expanded.hiddenMbCount).toBe(1);
  });

  it("summarizes mismatch warnings only for non-strict candidates and flags unverified tracks for download", () => {
    const strictCand = {
      source: { kind: "http_stream" as const, platform: "youtube", url: "https://yt/exact" },
      quality_tier: "high_lossy" as const,
      codec: "Opus",
      duration_ms: 156_000,
      match_confidence: 0.99,
      is_strict_match: true,
      display_label: "YouTube (Opus 160k)",
      matched_title: "Wild Child",
      matched_artist: "The Doors",
    };
    const fuzzyCand = {
      source: { kind: "http_stream" as const, platform: "youtube", url: "https://yt/edit" },
      quality_tier: "high_lossy" as const,
      codec: "Opus",
      duration_ms: 170_000,
      match_confidence: 0.72,
      is_strict_match: false,
      display_label: "YouTube (Opus 160k)",
      matched_title: "Wild Child (Single Edit)",
      matched_artist: "The Doors",
    };

    const t0 = track(0, { title: "Wild Child", duration_ms: 156_000, candidates: [strictCand], selected_candidate_idx: 0 });
    const t1 = track(1, { title: "Touch Me", duration_ms: 192_000, candidates: [fuzzyCand], selected_candidate_idx: 0 });

    expect(candidateMatchWarning(t0, strictCand)).toBeNull();
    expect(candidateMatchWarning(t1, fuzzyCand)).toContain("Wild Child (Single Edit)");
    expect(candidateMatchWarning(t1, fuzzyCand)).toContain("2:50 vs 3:12");

    const p = preview([t0, t1]);
    const unverified = unverifiedTracksForDownload(p);
    expect(unverified).toHaveLength(1);
    expect(unverified[0].track.index).toBe(1);
    expect(unverifiedTracksForDownload(p, [t0.url])).toEqual([]);
  });

  it("finds only downloaded tracks whose resolved candidate strictly improves local quality", () => {
    const losslessCand = {
      source: { kind: "http_stream" as const, platform: "bandcamp", url: "https://bc.bandcamp.com/track/1" },
      quality_tier: "lossless" as const,
      codec: "FLAC",
      duration_ms: 156_000,
      match_confidence: 0.99,
      is_strict_match: true,
      display_label: "Bandcamp · FLAC",
    };
    const highCand = {
      source: { kind: "http_stream" as const, platform: "youtube", url: "https://yt/exact" },
      quality_tier: "high_lossy" as const,
      codec: "Opus",
      duration_ms: 156_000,
      match_confidence: 0.99,
      is_strict_match: true,
      display_label: "YouTube · Opus 160k",
    };

    const t0 = track(0, { title: "Track 1", candidates: [losslessCand], selected_candidate_idx: 0 });
    const t1 = track(1, { title: "Track 2", candidates: [highCand], selected_candidate_idx: 0 });
    const t2 = track(2, { title: "Track 3", candidates: [losslessCand], selected_candidate_idx: 0 });

    const localTracks = {
      [t0.url]: { id: 101, quality_tier: "High" as const, path: "/m/1.opus" },
      [t1.url]: { id: 102, quality_tier: "High" as const, path: "/m/2.opus" },
      // t2 is not downloaded locally at all -> excluded from upgradableAlbumTracks
    };

    const upgradable = upgradableAlbumTracks(preview([t0, t1, t2]), localTracks);
    expect(upgradable.map((x) => x.local.id)).toEqual([101]);
    expect(upgradable[0].candidate.quality_tier).toBe("lossless");
  });
});
