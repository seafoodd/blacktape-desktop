import { describe, it, expect, vi, beforeEach } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { AlbumPreview, AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import { buildDownloadOverrides, useAlbumPreviewStore } from "./albumPreviewStore.ts";

type ActiveTask = { status: string; overallPercent: number };

const { getAlbumPreviewMock, getReleaseGroupReleasesMock, setActiveViewMock, startDownloadMock, activeDownloadsMock } = vi.hoisted(
  () => ({
    getAlbumPreviewMock: vi.fn(),
    getReleaseGroupReleasesMock: vi.fn().mockResolvedValue([]),
    setActiveViewMock: vi.fn(),
    startDownloadMock: vi.fn(),
    // Stands in for the library's `activeDownloads`: the startDownload mock registers tasks here.
    activeDownloadsMock: {} as Record<string, ActiveTask>,
  }),
);

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getReleaseGroupReleases: getReleaseGroupReleasesMock,
}));
vi.mock("@/shared/lib/remote/albumPreview.ts", async () => ({
  ...(await vi.importActual<typeof import("@/shared/lib/remote/albumPreview.ts")>(
    "@/shared/lib/remote/albumPreview.ts",
  )),
  getAlbumPreview: getAlbumPreviewMock,
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: {
    getState: () => ({
      setActiveView: setActiveViewMock,
    }),
    subscribe: vi.fn(() => () => {}),
  },
}));

const initialState = useAlbumPreviewStore.getState();
vi.mock("@/shared/store/downloadStore.ts", () => ({
  useDownloadStore: {
    getState: () => ({ startDownload: startDownloadMock, activeDownloads: activeDownloadsMock }),
  },
}));

const ALBUM_URL = "https://example.test/album";
const COVER_URL = "https://example.test/cover.jpg";

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

// Indexes 0, 1, 3, 4 are selectable; 2 is unavailable.
const makePreview = (): AlbumPreview => ({
  platform: "bandcamp",
  url: ALBUM_URL,
  title: "Album Title",
  artist: "Album Artist",
  cover_url: COVER_URL,
  year: 2020,
  genres: [],
  tracks: [
    track(0),
    track(1),
    track(2, { available: false, url: "", note: "Not for sale" }),
    track(3),
    track(4),
  ],
});

const albumItem = (over: Partial<SearchSuggestion> = {}): SearchSuggestion => ({
  item_type: ItemType.Album,
  name: "Album Title",
  band_name: "Album Artist",
  item_url_path: ALBUM_URL,
  img: COVER_URL,
  platform: "bandcamp",
  ...over,
});

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: unknown) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const store = () => useAlbumPreviewStore.getState();

/** Opens the page with a preview that is already loaded. */
async function openReady(preview: AlbumPreview = makePreview()) {
  getAlbumPreviewMock.mockResolvedValueOnce(preview);
  await store().open(albumItem());
  return preview;
}

let taskSeq = 0;

beforeEach(() => {
  getAlbumPreviewMock.mockReset();
  setActiveViewMock.mockReset();
  startDownloadMock.mockReset();
  // Like the real startDownload: registers the task synchronously, before anything awaits.
  startDownloadMock.mockImplementation(async () => {
    taskSeq += 1;
    activeDownloadsMock[`task-${taskSeq}`] = { status: "downloading", overallPercent: 0 };
  });
  taskSeq = 0;
  for (const id of Object.keys(activeDownloadsMock)) delete activeDownloadsMock[id];
  useAlbumPreviewStore.setState(initialState, true);
});

describe("open", () => {
  it("loads the tracks: loading first, then ready, and switches to the album view", async () => {
    const pending = deferred<AlbumPreview>();
    getAlbumPreviewMock.mockReturnValueOnce(pending.promise);
    const item = albumItem();

    const opening = store().open(item);

    expect(store().status).toBe("loading");
    expect(store().item).toBe(item);
    expect(store().preview).toBeNull();
    expect(setActiveViewMock).toHaveBeenCalledWith("ALBUM_PREVIEW");
    expect(getAlbumPreviewMock).toHaveBeenCalledWith("bandcamp", ALBUM_URL, undefined, {
      artist: item.band_name,
      title: item.name,
      coverUrl: item.img,
    });

    const preview = makePreview();
    pending.resolve(preview);
    await opening;

    expect(store().status).toBe("ready");
    expect(store().preview).toEqual(preview);
    expect(store().error).toBeNull();
  });

  it("clears the previous selection", async () => {
    useAlbumPreviewStore.setState({ selection: { selected: new Set([1, 3]), anchor: 3 } });

    await openReady();

    expect(store().selection.selected.size).toBe(0);
    expect(store().selection.anchor).toBeNull();
  });

  it("stores the error message when loading rejects with a string", async () => {
    getAlbumPreviewMock.mockRejectedValueOnce("Album not found");

    await store().open(albumItem());

    expect(store().status).toBe("error");
    expect(store().error).toBe("Album not found");
    expect(store().preview).toBeNull();
  });

  it("ignores the first response when a second open starts while it is pending", async () => {
    const first = deferred<AlbumPreview>();
    const second = deferred<AlbumPreview>();
    getAlbumPreviewMock.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const firstOpen = store().open(albumItem({ name: "first", item_url_path: "https://example.test/a" }));
    const secondOpen = store().open(albumItem({ name: "second", item_url_path: "https://example.test/b" }));

    const secondPreview = { ...makePreview(), title: "Second" };
    const firstPreview = { ...makePreview(), title: "First" };
    second.resolve(secondPreview);
    await secondOpen;
    first.resolve(firstPreview);
    await firstOpen;

    expect(store().status).toBe("ready");
    expect(store().preview).toEqual(secondPreview);
    expect(store().item?.name).toBe("second");
  });

  it("ignores a failure of the first open after a newer open succeeded", async () => {
    const first = deferred<AlbumPreview>();
    const second = deferred<AlbumPreview>();
    getAlbumPreviewMock.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    const firstOpen = store().open(albumItem());
    const secondOpen = store().open(albumItem());
    second.resolve(makePreview());
    await secondOpen;
    first.reject("stale failure");
    await firstOpen;

    expect(store().status).toBe("ready");
    expect(store().error).toBeNull();
  });
});

describe("close", () => {
  it("switches back to the search results", () => {
    store().close();
    expect(setActiveViewMock).toHaveBeenCalledWith("SEARCH_RESULTS");
  });

  it("ignores a response that is still in flight", async () => {
    const pending = deferred<AlbumPreview>();
    getAlbumPreviewMock.mockReturnValueOnce(pending.promise);
    const opening = store().open(albumItem());

    store().close();
    pending.resolve(makePreview());
    await opening;

    expect(store().status).toBe("loading");
    expect(store().preview).toBeNull();
  });
});

describe("selectTrack", () => {
  beforeEach(async () => {
    await openReady();
  });

  it("a plain click selects only that track", () => {
    store().selectTrack(1, { ctrl: false, shift: false });
    store().selectTrack(3, { ctrl: false, shift: false });
    expect([...store().selection.selected]).toEqual([3]);
  });

  it("Ctrl adds a track to the selection", () => {
    store().selectTrack(1, { ctrl: false, shift: false });
    store().selectTrack(4, { ctrl: true, shift: false });
    expect([...store().selection.selected].sort()).toEqual([1, 4]);
  });

  it("Shift selects the range from the anchor, using selectable tracks only", () => {
    store().selectTrack(1, { ctrl: false, shift: false });
    store().selectTrack(4, { ctrl: false, shift: true });
    // Album order of selectable tracks is 0, 1, 3, 4: the range from 1 to 4 skips the unavailable 2.
    expect([...store().selection.selected].sort()).toEqual([1, 3, 4]);
  });

  it("clicking an unavailable track does nothing", () => {
    store().selectTrack(0, { ctrl: false, shift: false });
    const before = store().selection;

    store().selectTrack(2, { ctrl: true, shift: false });

    expect(store().selection).toBe(before);
    expect([...store().selection.selected]).toEqual([0]);
  });

  it("does nothing without a loaded preview", () => {
    useAlbumPreviewStore.setState({ preview: null });
    store().selectTrack(0, { ctrl: false, shift: false });
    expect(store().selection.selected.size).toBe(0);
  });
});

describe("selectAll and clearSelection", () => {
  it("selectAll selects every selectable track and no unavailable one", async () => {
    await openReady();

    store().selectAll();

    expect([...store().selection.selected].sort()).toEqual([0, 1, 3, 4]);
  });

  it("clearSelection empties the selection", async () => {
    await openReady();
    store().selectAll();

    store().clearSelection();

    expect(store().selection.selected.size).toBe(0);
    expect(store().selection.anchor).toBeNull();
  });
});

describe("download", () => {
  it("with nothing selected downloads the whole album", async () => {
    await openReady();

    await store().download();

    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock).toHaveBeenCalledWith(
      expect.objectContaining({
        platform: "bandcamp",
        downloadType: "Album",
        url: ALBUM_URL,
        artistName: "Album Artist",
        title: "Album Title",
        coverUrl: COVER_URL,
        trackUrls: undefined,
      }),
    );
  });

  it("with some tracks selected passes their urls in album order", async () => {
    await openReady();
    store().selectTrack(4, { ctrl: false, shift: false });
    store().selectTrack(0, { ctrl: true, shift: false });

    await store().download();

    expect(startDownloadMock).toHaveBeenCalledWith(
      expect.objectContaining({
        downloadType: "Album",
        url: ALBUM_URL,
        trackUrls: ["https://example.test/track/0", "https://example.test/track/4"],
      }),
    );
  });

  it("with every available track selected downloads the whole album", async () => {
    await openReady();
    store().selectAll();

    await store().download();

    expect(startDownloadMock).toHaveBeenCalledWith(
      expect.objectContaining({ trackUrls: undefined }),
    );
  });

  it("does nothing without a loaded preview", async () => {
    await store().download();
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("the selected rows are the ones the task serves", async () => {
    await openReady();
    store().selectTrack(4, { ctrl: false, shift: false });
    store().selectTrack(0, { ctrl: true, shift: false });

    await store().download();

    expect(store().trackTasks).toEqual({
      "task-1": ["https://example.test/track/0", "https://example.test/track/4"],
    });
  });

  it("the whole album serves every selectable row, not the unavailable one", async () => {
    await openReady();

    await store().download();

    expect(store().trackTasks).toEqual({
      "task-1": [
        "https://example.test/track/0",
        "https://example.test/track/1",
        "https://example.test/track/3",
        "https://example.test/track/4",
      ],
    });
  });
});

describe("downloadTrack", () => {
  it("downloads the one track through the album pipeline", async () => {
    await openReady();

    await store().downloadTrack(track(3));

    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock).toHaveBeenCalledWith(
      expect.objectContaining({
        platform: "bandcamp",
        downloadType: "Album",
        url: ALBUM_URL,
        artistName: "Album Artist",
        title: "Album Title",
        coverUrl: COVER_URL,
        trackUrls: ["https://example.test/track/3"],
      }),
    );
  });

  it("registers the task against that track's row only", async () => {
    await openReady();

    await store().downloadTrack(track(3));

    expect(store().trackTasks).toEqual({ "task-1": ["https://example.test/track/3"] });
  });

  it("does nothing for an unavailable track", async () => {
    await openReady();

    await store().downloadTrack(track(2, { available: false, url: "" }));

    expect(startDownloadMock).not.toHaveBeenCalled();
    expect(store().trackTasks).toEqual({});
  });

  it("does nothing without a loaded preview", async () => {
    await store().downloadTrack(track(0));
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("forgets tasks the library no longer knows when a new download starts", async () => {
    await openReady();
    useAlbumPreviewStore.setState({
      trackTasks: { finishedLongAgo: ["https://example.test/track/0"] },
    });

    await store().downloadTrack(track(3));

    expect(Object.keys(store().trackTasks)).toEqual(["task-1"]);
  });

  it("keeps the tasks the library still knows", async () => {
    await openReady();
    activeDownloadsMock.stillRunning = { status: "processing", overallPercent: 10 };
    useAlbumPreviewStore.setState({
      trackTasks: { stillRunning: ["https://example.test/track/0"] },
    });

    await store().downloadTrack(track(3));

    expect(store().trackTasks).toEqual({
      stillRunning: ["https://example.test/track/0"],
      "task-1": ["https://example.test/track/3"],
    });
  });
});

describe("download formats", () => {
  it("download(format) passes the picked format to startDownload", async () => {
    await openReady();
    await store().download("251");
    expect(startDownloadMock.mock.calls[0][0].format).toBe("251");
  });

  it("download() without a format passes none", async () => {
    await openReady();
    await store().download();
    expect(startDownloadMock.mock.calls[0][0].format).toBeUndefined();
  });

  it("download(format) keeps the selection of the tracks it downloads", async () => {
    await openReady();
    store().selectTrack(0, { ctrl: false, shift: false });
    store().selectTrack(3, { ctrl: true, shift: false });
    await store().download("140");
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/3",
    ]);
    expect(startDownloadMock.mock.calls[0][0].format).toBe("140");
  });

  it("downloadTrack(track, format) passes the format", async () => {
    await openReady();
    await store().downloadTrack(track(3), "mp3-320");
    expect(startDownloadMock.mock.calls[0][0].format).toBe("mp3-320");
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual(["https://example.test/track/3"]);
  });

  it("downloadUrls(urls, format) passes the format with the given urls", async () => {
    await openReady();
    await store().downloadUrls(["https://example.test/track/1"], "flac");
    expect(startDownloadMock.mock.calls[0][0].format).toBe("flac");
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual(["https://example.test/track/1"]);
  });

  it("buildDownloadOverrides forwards edited metadata and chosen candidate url + fallbacks", () => {
    const preview: AlbumPreview = {
      ...makePreview(),
      title: "Edited Album",
      artist: "Edited Artist",
      year: 2001,
      tracks: [
        {
          index: 0,
          title: "Edited Track",
          artists: ["Edited Artist"],
          track_number: 1,
          url: "https://picked/1",
          available: true,
          note: null,
          selected_candidate_idx: 1,
          candidates: [
            {
              source: { kind: "http_stream", platform: "youtube", url: "https://yt/1" },
              quality_tier: "high_lossy",
              codec: "Opus",
              bitrate_kbps: 160,
              duration_ms: 180_000,
              match_confidence: 0.9,
              is_strict_match: true,
              display_label: "YT",
            },
            {
              source: { kind: "http_stream", platform: "bandcamp", url: "https://bc/1" },
              quality_tier: "lossless",
              codec: "FLAC",
              duration_ms: 180_000,
              match_confidence: 0.95,
              is_strict_match: true,
              display_label: "BC",
            },
          ],
        },
      ],
    };

    const ov = buildDownloadOverrides(preview);
    expect(ov).toEqual({
      title: "Edited Album",
      artist: "Edited Artist",
      year: 2001,
      tracks: [
        {
          index: 0,
          title: "Edited Track",
          original_url: "https://picked/1",
          url: "https://bc/1",
          fallback_urls: ["https://yt/1"],
          candidates: preview.tracks[0].candidates,
          selected_candidate_idx: 1,
        },
      ],
    });
  });

  it("loads release-group editions for MusicBrainz albums and switches releases cleanly", async () => {
    const mbPreview = {
      ...makePreview(),
      url: "canonical:rg:rg-1",
      metadata_source: "musicbrainz",
      release_group_id: "rg-1",
      selected_release_id: "rel-30",
    };
    const expandedPreview = {
      ...mbPreview,
      url: "canonical:rel-35",
      selected_release_id: "rel-35",
      tracks: [track(0, { title: "Bonus 1" })],
    };
    const editions = [
      { id: "rel-30", title: "Drukqs", edition_label: "Original edition", year: 2001, track_count: 30, disc_count: 1, format: "Digital Media", is_default: true },
      { id: "rel-35", title: "Drukqs", edition_label: "Expanded edition", year: 2026, track_count: 35, disc_count: 1, format: "Digital Media", is_default: false },
    ];
    getAlbumPreviewMock.mockResolvedValueOnce(mbPreview);
    getReleaseGroupReleasesMock.mockResolvedValueOnce(editions);

    await store().open(albumItem());
    await new Promise((r) => setTimeout(r, 0));
    expect(getReleaseGroupReleasesMock).toHaveBeenCalledWith("rg-1", "Album Artist", "Album Title");
    expect(store().editions).toEqual(editions);

    getAlbumPreviewMock.mockResolvedValueOnce(expandedPreview);
    await store().selectRelease("rel-35");
    expect(getAlbumPreviewMock).toHaveBeenLastCalledWith("bandcamp", "canonical:rel-35", false);
    expect(store().preview?.selected_release_id).toBe("rel-35");
    expect(store().preview?.release_group_id).toBe("rg-1");
    expect(store().editions).toEqual(editions);
  });

  it("switches to a platform edition with skipMbUpgrade while keeping release_group_id and editions", async () => {
    const mbPreview = {
      ...makePreview(),
      url: "canonical:rg:rg-1",
      metadata_source: "musicbrainz",
      release_group_id: "rg-1",
      selected_release_id: "rel-30",
    };
    const ytPreview = {
      ...makePreview(),
      platform: "youtube",
      url: "https://music.youtube.com/playlist?list=PL123",
      metadata_source: "youtube",
      tracks: [track(0, { title: "YT Track 1" })],
    };
    const editions = [
      { id: "rel-30", title: "Drukqs", edition_label: "Original edition", year: 2001, track_count: 30, disc_count: 1, format: "Digital Media", is_default: true, source_kind: "musicbrainz", url: "canonical:rel-30" },
      { id: "platform:youtube:https://music.youtube.com/playlist?list=PL123", title: "Drukqs", edition_label: "YouTube Music", year: 2001, track_count: 30, disc_count: 1, format: "YouTube Music", is_default: false, source_kind: "youtube", url: "https://music.youtube.com/playlist?list=PL123" },
    ];
    getAlbumPreviewMock.mockResolvedValueOnce(mbPreview);
    getReleaseGroupReleasesMock.mockResolvedValueOnce(editions);

    await store().open(albumItem());
    await new Promise((r) => setTimeout(r, 0));

    getAlbumPreviewMock.mockResolvedValueOnce(ytPreview);
    await store().selectRelease(editions[1]);
    expect(getAlbumPreviewMock).toHaveBeenLastCalledWith(
      "youtube",
      "https://music.youtube.com/playlist?list=PL123",
      true,
    );
    expect(store().preview?.platform).toBe("youtube");
    expect(store().preview?.release_group_id).toBe("rg-1");
    expect(store().editions).toEqual(editions);
  });

  it("does not stick resolvingCandidates=true when backend returns candidates_resolved=true", async () => {
    const resolvedPreview = {
      ...makePreview(),
      tracks: [track(0, { candidates: [], available: true })],
      url: "https://example.test/cached-album",
      candidates_resolved: true,
    };
    getAlbumPreviewMock.mockResolvedValueOnce(resolvedPreview);

    await store().open(albumItem({ item_url_path: resolvedPreview.url }));
    expect(store().status).toBe("ready");
    expect(store().resolvingCandidates).toBe(false);
  });

  it("keeps the selected torrent when streaming results arrive after restore", async () => {
    const torrent: NonNullable<AlbumTrackInfo["candidates"]>[number] = {
      source: {
        kind: "torrent", info_hash: "test-hash", magnet_uri: "magnet:?xt=urn:btih:test-hash",
        file_index: 0, file_path: "01 Track One.flac", file_size: 20_000_000, seeders: 3,
      },
      quality_tier: "lossless", codec: "FLAC", duration_ms: 180_000,
      match_confidence: 1, is_strict_match: true, display_label: "Torrent FLAC",
    };
    const stream: typeof torrent = {
      ...torrent, source: { kind: "http_stream", platform: "youtube", url: "https://yt/1" },
      quality_tier: "high_lossy", codec: "Opus", display_label: "YT · Opus",
    };
    const preview: AlbumPreview = {
      ...makePreview(), url: "canonical:album:progressive-torrent",
      metadata_source: "musicbrainz", candidates_resolved: false,
      tracks: [track(0, { candidates: [torrent], selected_candidate_idx: 0 })],
    };
    useAlbumPreviewStore.setState({ status: "ready", preview, resolvingCandidates: true });
    getAlbumPreviewMock.mockResolvedValueOnce({
      ...preview, candidates_resolved: true, tracks: [track(0, { candidates: [stream] })],
    });
    await store().resumeAfterRestore();
    expect(store().preview?.tracks[0]).toMatchObject({
      candidates: [stream, torrent], selected_candidate_idx: 1,
    });
    expect(store().resolvingCandidates).toBe(false);
  });

  it("resumeAfterRestore clears resolvingCandidates and fetches missing editions when cache finished while away", async () => {
    const freshWithCandidates = {
      ...makePreview(),
      tracks: [track(0, { candidates: [{ source: { kind: "http_stream", platform: "youtube", url: "https://yt/1" }, quality_tier: "high_lossy", codec: "Opus", duration_ms: 180_000, match_confidence: 1, is_strict_match: true, display_label: "YT · Opus" }] })],
      url: "canonical:rg:rg-resume",
      release_group_id: "rg-resume",
      candidates_resolved: true,
    };
    const editions = [
      { id: "rel-1", title: "Album Title", edition_label: "Original edition", year: 2020, track_count: 1, disc_count: 1, format: "CD", is_default: true },
    ];
    useAlbumPreviewStore.setState({
      item: albumItem(),
      status: "ready",
      preview: {
        ...makePreview(),
        tracks: [track(0, { candidates: [] })],
        url: "canonical:rg:rg-resume",
        release_group_id: "rg-resume",
        metadata_source: "musicbrainz",
        candidates_resolved: false,
      },
      editions: [],
      resolvingCandidates: true,
    });
    getReleaseGroupReleasesMock.mockResolvedValueOnce(editions);
    getAlbumPreviewMock.mockResolvedValueOnce(freshWithCandidates);

    await store().resumeAfterRestore();
    await new Promise((r) => setTimeout(r, 0));

    expect(store().resolvingCandidates).toBe(false);
    expect(store().preview?.tracks[0].candidates?.length).toBe(1);
    expect(store().editions).toEqual(editions);
  });

  it("invalidatePending drops in-flight open responses from an aborted forward navigation", async () => {
    const pendingOpen = deferred<AlbumPreview>();
    getAlbumPreviewMock.mockReturnValueOnce(pendingOpen.promise);

    const openPromise = store().open(albumItem({ item_url_path: "https://example.test/forward-album" }));
    store().invalidatePending();
    useAlbumPreviewStore.setState({
      status: "ready",
      preview: { ...makePreview(), url: "https://example.test/restored-album", title: "Restored" },
    });

    pendingOpen.resolve({ ...makePreview(), url: "https://example.test/forward-album", title: "Stale Forward" });
    await openPromise;

    expect(store().preview?.title).toBe("Restored");
  });

  it("offers late MusicBrainz editions without replacing the open preview or its selection", async () => {
    const ytPreview = {
      ...makePreview(),
      platform: "youtube" as const,
      url: "https://music.youtube.com/browse/MPREb_soft_parade",
      title: "The Soft Parade",
      artist: "The Doors",
      metadata_source: "youtube",
      release_group_id: null,
      selected_release_id: null,
      candidates_resolved: true,
    };
    const mbUpgraded = {
      ...ytPreview,
      url: "canonical:rel-soft-parade",
      metadata_source: "musicbrainz",
      release_group_id: "rg-soft-parade",
      selected_release_id: "rel-soft-parade",
      candidates_resolved: true,
    };
    const editions = [
      {
        id: "rel-soft-parade",
        title: "The Soft Parade",
        edition_label: "Original edition",
        year: 1969,
        track_count: 9,
        disc_count: 1,
        format: "CD",
        is_default: true,
      },
      {
        id: "yt:https://music.youtube.com/browse/MPREb_soft_parade",
        title: "The Soft Parade",
        edition_label: "YouTube Music",
        year: 2006,
        track_count: 0,
        disc_count: 1,
        format: "Opus High",
        is_default: false,
        source_kind: "youtube" as const,
        url: "https://music.youtube.com/browse/MPREb_soft_parade",
      },
    ];

    getAlbumPreviewMock.mockResolvedValueOnce(ytPreview).mockResolvedValueOnce(mbUpgraded);
    const pending = deferred<typeof editions>();
    getReleaseGroupReleasesMock.mockReturnValueOnce(pending.promise);

    await store().open(
      albumItem({
        platform: "youtube",
        name: "The Soft Parade",
        band_name: "The Doors",
        item_url_path: ytPreview.url,
      }),
    );
    expect(store().loadingEditions).toBe(true);
    store().selectTrack(0, { ctrl: false, shift: false });
    const selection = store().selection;
    pending.resolve(editions);
    await pending.promise;
    await Promise.resolve();

    expect(getReleaseGroupReleasesMock).toHaveBeenCalledWith(null, "The Doors", "The Soft Parade");
    expect(getAlbumPreviewMock).toHaveBeenCalledTimes(1);
    expect(store().loadingEditions).toBe(false);
    expect(store().preview?.metadata_source).toBe("youtube");
    expect(store().selection).toBe(selection);
    expect(store().editions).toEqual(editions);

    await store().selectRelease("rel-soft-parade");
    expect(getAlbumPreviewMock).toHaveBeenNthCalledWith(2, "youtube", "canonical:rel-soft-parade", false);
    expect(store().preview?.metadata_source).toBe("musicbrainz");
    expect(store().preview?.selected_release_id).toBe("rel-soft-parade");
    expect(store().editions).toEqual(editions);
  });

  it("resumeAfterRestore does not override a manually chosen platform edition back to MusicBrainz", async () => {
    const ytManual = {
      ...makePreview(),
      platform: "youtube" as const,
      url: "https://music.youtube.com/browse/MPREb_soft_parade",
      title: "The Soft Parade",
      artist: "The Doors",
      metadata_source: "youtube",
      release_group_id: "rg-soft-parade",
      selected_release_id: "yt:https://music.youtube.com/browse/MPREb_soft_parade",
      candidates_resolved: true,
      tracks: [track(0, { candidates: [] })],
    };
    const editions = [
      {
        id: "rel-soft-parade",
        title: "The Soft Parade",
        edition_label: "Original edition",
        year: 1969,
        track_count: 9,
        disc_count: 1,
        format: "CD",
        is_default: true,
      },
      {
        id: "yt:https://music.youtube.com/browse/MPREb_soft_parade",
        title: "The Soft Parade",
        edition_label: "YouTube Music",
        year: 2006,
        track_count: 0,
        disc_count: 1,
        format: "Opus High",
        is_default: false,
        source_kind: "youtube" as const,
        url: "https://music.youtube.com/browse/MPREb_soft_parade",
      },
    ];
    useAlbumPreviewStore.setState({
      item: albumItem({ platform: "youtube", item_url_path: ytManual.url }),
      status: "ready",
      preview: ytManual,
      editions,
      resolvingCandidates: false,
    });

    await store().resumeAfterRestore();
    await new Promise((r) => setTimeout(r, 0));

    expect(getAlbumPreviewMock).not.toHaveBeenCalled();
    expect(store().preview?.metadata_source).toBe("youtube");
  });
});
