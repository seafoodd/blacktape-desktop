import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { LibraryAlbumSource, MissingTracks } from "@/shared/lib/libraryIndex.ts";
import {
  busyAlbumUrls,
  failedSongs,
  recoveredSongs,
  withoutUnavailable,
  missingDownloadParams,
  missingTrackUrls,
  playableMissingTracks,
  prunePending,
  pruneShown,
  sourceOfAlbum,
  sourcesByAlbum,
  toggleShown,
  useMissingTracksStore,
} from "./missingTracksStore.ts";
import { isNoStreamFailure } from "./downloadFailure.ts";

const { getArtistAlbumSourcesMock, getMissingTracksMock, startDownloadMock, subscribeMock } =
  vi.hoisted(() => ({
    getArtistAlbumSourcesMock: vi.fn(),
    getMissingTracksMock: vi.fn(),
    startDownloadMock: vi.fn(),
    subscribeMock: vi.fn(),
  }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/libraryIndex.ts", () => ({
  getArtistAlbumSources: getArtistAlbumSourcesMock,
  getMissingTracks: getMissingTracksMock,
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: {
    getState: () => ({ startDownload: startDownloadMock }),
    subscribe: subscribeMock,
  },
}));

/** The store's subscription to the library downloads (registered once, when the store module loads). */
const onLibraryChange = subscribeMock.mock.calls[0][0] as (state: {
  activeDownloads: Record<string, { url: string; status: string }>;
}) => void;

const source = (album: string, over: Partial<LibraryAlbumSource> = {}): LibraryAlbumSource => ({
  album,
  platform: "bandcamp",
  source_url: `https://example.test/album/${album}`,
  cover_url: null,
  total: 3,
  owned: 1,
  missing: 2,
  ...over,
});

const track = (index: number, over: Partial<AlbumTrackInfo> = {}): AlbumTrackInfo => ({
  index,
  title: `Track ${index + 1}`,
  artists: ["Artist"],
  track_number: index + 1,
  url: `https://example.test/track/${index}`,
  available: true,
  note: null,
  duration_ms: null,
  ...over,
});

const missing = (over: Partial<MissingTracks> = {}): MissingTracks => ({
  platform: "bandcamp",
  source_url: "https://example.test/album/A",
  album_title: "A",
  album_artist: "Artist",
  cover_url: "https://example.test/cover.jpg",
  tracks: [track(1), track(2)],
  ...over,
});

/** A promise the test settles by hand, to control the order of responses. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Lets the pending store actions run to their next await (no timers). */
async function flush() {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

beforeEach(() => {
  vi.clearAllMocks();
  useMissingTracksStore.setState({
    artist: null,
    sources: {},
    shown: new Set(),
    tracks: {},
    pendingUrls: new Map(),
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("sourcesByAlbum", () => {
  it("keys the sources by library album title", () => {
    const sources = sourcesByAlbum([source("A"), source("B", { missing: 0 })]);
    expect(Object.keys(sources).sort()).toEqual(["A", "B"]);
    expect(sources.B.missing).toBe(0);
  });

  it("keeps the last source when two rows share a title", () => {
    const sources = sourcesByAlbum([source("A", { missing: 1 }), source("A", { missing: 4 })]);
    expect(sources.A.missing).toBe(4);
  });
});

describe("sourceOfAlbum", () => {
  it("finds the source of an album by its title", () => {
    const sources = sourcesByAlbum([source("A")]);
    expect(sourceOfAlbum(sources, "A")?.album).toBe("A");
    expect(sourceOfAlbum(sources, "B")).toBeUndefined();
  });

  it("does not return inherited object members for an album titled like one", () => {
    expect(sourceOfAlbum({}, "constructor")).toBeUndefined();
    expect(sourceOfAlbum(sourcesByAlbum([source("A")]), "toString")).toBeUndefined();
  });
});

describe("toggleShown", () => {
  it("adds an album that is not shown and removes one that is", () => {
    const shown = new Set(["A"]);
    expect([...toggleShown(shown, "B")].sort()).toEqual(["A", "B"]);
    expect([...toggleShown(shown, "A")]).toEqual([]);
  });

  it("returns a new set and leaves the input untouched", () => {
    const shown = new Set(["A"]);
    const next = toggleShown(shown, "B");
    expect(next).not.toBe(shown);
    expect([...shown]).toEqual(["A"]);
  });
});

describe("pruneShown", () => {
  it("keeps only the shown albums that still have a source", () => {
    const sources = sourcesByAlbum([source("A")]);
    expect([...pruneShown(new Set(["A", "Gone"]), sources)]).toEqual(["A"]);
  });

  it("does not treat inherited object keys as sources", () => {
    expect([...pruneShown(new Set(["constructor"]), {})]).toEqual([]);
  });
});

describe("playableMissingTracks and missingTrackUrls", () => {
  it("drops unavailable tracks and tracks without a url", () => {
    const list = [
      track(0),
      track(1, { available: false, url: "" }),
      track(2, { url: "" }),
      track(3),
    ];
    expect(playableMissingTracks({ tracks: list }).map((t) => t.index)).toEqual([0, 3]);
    expect(missingTrackUrls({ tracks: list })).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/3",
    ]);
  });
});

describe("missingDownloadParams", () => {
  it("builds an album download of the given tracks, titled like the library album", () => {
    const params = missingDownloadParams(missing(), ["https://example.test/track/1"]);
    expect(params).toEqual({
      platform: "bandcamp",
      downloadType: "Album",
      url: "https://example.test/album/A",
      artistName: "Artist",
      title: "A",
      coverUrl: "https://example.test/cover.jpg",
      trackUrls: ["https://example.test/track/1"],
    });
  });

  it("leaves the cover out when the album has none", () => {
    const params = missingDownloadParams(missing({ cover_url: null }), []);
    expect(params.coverUrl).toBeUndefined();
  });
});

describe("loadSources", () => {
  it("stores the sources of the artist by album title", async () => {
    getArtistAlbumSourcesMock.mockResolvedValue([source("A")]);
    await useMissingTracksStore.getState().loadSources("Artist");
    const state = useMissingTracksStore.getState();
    expect(getArtistAlbumSourcesMock).toHaveBeenCalledWith("Artist");
    expect(state.artist).toBe("Artist");
    expect(state.sources.A.missing).toBe(2);
  });

  it("forgets the shown albums and the tracks of the previous artist", async () => {
    useMissingTracksStore.setState({
      artist: "Old",
      shown: new Set(["A"]),
      tracks: { A: missing() },
      sources: sourcesByAlbum([source("A")]),
    });
    getArtistAlbumSourcesMock.mockResolvedValue([]);
    await useMissingTracksStore.getState().loadSources("New");
    const state = useMissingTracksStore.getState();
    expect(state.artist).toBe("New");
    expect(state.shown.size).toBe(0);
    expect(state.tracks).toEqual({});
    expect(state.sources).toEqual({});
  });

  it("ignores a late answer for an artist that is no longer selected", async () => {
    const first = deferred<LibraryAlbumSource[]>();
    getArtistAlbumSourcesMock.mockReturnValueOnce(first.promise);
    getArtistAlbumSourcesMock.mockResolvedValueOnce([source("B")]);

    const late = useMissingTracksStore.getState().loadSources("A");
    await useMissingTracksStore.getState().loadSources("B");
    first.resolve([source("A")]);
    await late;

    const state = useMissingTracksStore.getState();
    expect(state.artist).toBe("B");
    expect(Object.keys(state.sources)).toEqual(["B"]);
  });

  it("drops shown albums that lost their source", async () => {
    getArtistAlbumSourcesMock.mockResolvedValueOnce([source("A")]);
    await useMissingTracksStore.getState().loadSources("Artist");
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.getState().toggle("A");
    await flush();

    getArtistAlbumSourcesMock.mockResolvedValueOnce([]);
    await useMissingTracksStore.getState().loadSources("Artist");
    expect(useMissingTracksStore.getState().shown.has("A")).toBe(false);
  });

  it("re-reads the missing songs of the shown albums (a download may have added some)", async () => {
    getArtistAlbumSourcesMock.mockResolvedValue([source("A")]);
    getMissingTracksMock.mockResolvedValue(missing());
    await useMissingTracksStore.getState().loadSources("Artist");
    useMissingTracksStore.getState().toggle("A");
    await flush();
    expect(getMissingTracksMock).toHaveBeenCalledTimes(1);

    getMissingTracksMock.mockResolvedValue(missing({ tracks: [track(2)] }));
    await useMissingTracksStore.getState().loadSources("Artist");
    await flush();
    expect(getMissingTracksMock).toHaveBeenCalledTimes(2);
    expect(useMissingTracksStore.getState().tracks.A?.tracks.map((t) => t.index)).toEqual([2]);
  });

  it("keeps the previous sources when the backend call fails", async () => {
    getArtistAlbumSourcesMock.mockResolvedValueOnce([source("A")]);
    await useMissingTracksStore.getState().loadSources("Artist");
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    getArtistAlbumSourcesMock.mockRejectedValueOnce("db locked");
    await useMissingTracksStore.getState().loadSources("Artist");
    expect(error).toHaveBeenCalled();
    expect(Object.keys(useMissingTracksStore.getState().sources)).toEqual(["A"]);
  });
});

describe("toggle", () => {
  it("shows an album and loads its missing songs", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    useMissingTracksStore.getState().toggle("A");
    expect(useMissingTracksStore.getState().shown.has("A")).toBe(true);
    await flush();
    expect(getMissingTracksMock).toHaveBeenCalledWith("Artist", "A");
    expect(useMissingTracksStore.getState().tracks.A?.tracks).toHaveLength(2);
  });

  it("hides an album again without reading anything", () => {
    useMissingTracksStore.setState({ artist: "Artist", shown: new Set(["A"]) });
    useMissingTracksStore.getState().toggle("A");
    expect(useMissingTracksStore.getState().shown.has("A")).toBe(false);
    expect(getMissingTracksMock).not.toHaveBeenCalled();
  });

  it("drops a late answer that arrives after the artist changed", async () => {
    const pending = deferred<MissingTracks | null>();
    getMissingTracksMock.mockReturnValueOnce(pending.promise);
    useMissingTracksStore.setState({ artist: "Artist" });
    useMissingTracksStore.getState().toggle("A");

    useMissingTracksStore.setState({ artist: "Other", tracks: {} });
    pending.resolve(missing());
    await flush();
    expect(useMissingTracksStore.getState().tracks.A).toBeUndefined();
  });

  it("stores null for an album without a source", async () => {
    getMissingTracksMock.mockResolvedValue(null);
    useMissingTracksStore.setState({ artist: "Artist" });
    useMissingTracksStore.getState().toggle("A");
    await flush();
    expect(useMissingTracksStore.getState().tracks.A).toBeNull();
  });
});

describe("downloadMissing", () => {
  it("starts one album download with the urls of all missing songs", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock).toHaveBeenCalledWith({
      platform: "bandcamp",
      downloadType: "Album",
      url: "https://example.test/album/A",
      artistName: "Artist",
      title: "A",
      coverUrl: "https://example.test/cover.jpg",
      trackUrls: ["https://example.test/track/1", "https://example.test/track/2"],
    });
  });

  it("reads the missing songs fresh before downloading", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist", tracks: { A: null } });
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(getMissingTracksMock).toHaveBeenCalledWith("Artist", "A");
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
  });

  it("does nothing when no song is missing any more", async () => {
    getMissingTracksMock.mockResolvedValue(missing({ tracks: [] }));
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("does nothing for an album without a source", async () => {
    getMissingTracksMock.mockResolvedValue(null);
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("does nothing when the backend read fails", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    getMissingTracksMock.mockRejectedValue("boom");
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).not.toHaveBeenCalled();
  });
});

describe("downloadOne", () => {
  it("starts an album download of only the chosen song", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual([
      "https://example.test/track/2",
    ]);
  });

  it("ignores a song that is already in the library", async () => {
    getMissingTracksMock.mockResolvedValue(missing({ tracks: [track(1)] }));
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("does not start a second download of a song that is already downloading", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
  });

  it("starts one download for two clicks made before the first read finished", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    const store = useMissingTracksStore.getState();
    await Promise.all([store.downloadOne("A", track(2)), store.downloadOne("A", track(2))]);
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
  });

  it("marks the song as pending while its download runs", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    expect(useMissingTracksStore.getState().pendingUrls.get("https://example.test/track/2")).toBe(
      "https://example.test/album/A",
    );
  });
});

describe("download missing and pending songs", () => {
  it("leaves out the songs that are already downloading", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(1));
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).toHaveBeenCalledTimes(2);
    expect(startDownloadMock.mock.calls[1][0].trackUrls).toEqual([
      "https://example.test/track/2",
    ]);
  });

  it("starts nothing when every missing song is already downloading", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMissing("A");
    await useMissingTracksStore.getState().downloadMissing("A");
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
  });

  it("releases a pending song once its album download is no longer busy", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    const url = "https://example.test/track/2";
    const albumUrl = "https://example.test/album/A";

    onLibraryChange({ activeDownloads: { t1: { url: albumUrl, status: "downloading" } } });
    expect(useMissingTracksStore.getState().pendingUrls.has(url)).toBe(true);

    onLibraryChange({ activeDownloads: { t1: { url: albumUrl, status: "failed" } } });
    expect(useMissingTracksStore.getState().pendingUrls.has(url)).toBe(false);
  });

  it("releases pending songs when the download task is gone", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadOne("A", track(2));
    onLibraryChange({ activeDownloads: {} });
    expect(useMissingTracksStore.getState().pendingUrls.size).toBe(0);
  });

  it("keeps the pending song while another task on the same album is still busy", () => {
    useMissingTracksStore.setState({
      pendingUrls: new Map([["https://example.test/track/2", "https://example.test/album/A"]]),
    });
    onLibraryChange({
      activeDownloads: {
        done: { url: "https://example.test/album/A", status: "completed" },
        live: { url: "https://example.test/album/A", status: "processing" },
      },
    });
    expect(useMissingTracksStore.getState().pendingUrls.size).toBe(1);
  });
});

describe("busyAlbumUrls", () => {
  it("counts running, queued and processing downloads only", () => {
    const busy = busyAlbumUrls({
      a: { url: "u1", status: "downloading" },
      b: { url: "u2", status: "idle" },
      c: { url: "u3", status: "processing" },
      d: { url: "u4", status: "completed" },
      e: { url: "u5", status: "failed" },
      f: { url: "u6", status: "cancelled" },
    });
    expect([...busy].sort()).toEqual(["u1", "u2", "u3"]);
  });
});

describe("prunePending", () => {
  it("keeps the entries whose album is busy and returns a new map", () => {
    const pending = new Map([
      ["t1", "albumA"],
      ["t2", "albumB"],
    ]);
    const next = prunePending(pending, new Set(["albumB"]));
    expect([...next]).toEqual([["t2", "albumB"]]);
    expect(pending.size).toBe(2);
  });
});

describe("downloadMany", () => {
  it("starts ONE download containing only the requested tracks that are still missing", async () => {
    getMissingTracksMock.mockResolvedValue(missing({ tracks: [track(1), track(2), track(3)] }));
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMany("A", [track(1), track(2)]);
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual([
      "https://example.test/track/1",
      "https://example.test/track/2",
    ]);
  });

  it("skips tracks already pending (pendingUrls)", async () => {
    getMissingTracksMock.mockResolvedValue(missing());
    useMissingTracksStore.setState({
      artist: "Artist",
      pendingUrls: new Map([["https://example.test/track/1", "https://example.test/album/A"]]),
    });
    await useMissingTracksStore.getState().downloadMany("A", [track(1), track(2)]);
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual([
      "https://example.test/track/2",
    ]);
  });

  it("skips tracks that are no longer missing", async () => {
    getMissingTracksMock.mockResolvedValue(missing({ tracks: [track(1)] }));
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMany("A", [track(1), track(2)]);
    expect(startDownloadMock).toHaveBeenCalledTimes(1);
    expect(startDownloadMock.mock.calls[0][0].trackUrls).toEqual([
      "https://example.test/track/1",
    ]);
  });

  it("does nothing when nothing remains to download", async () => {
    getMissingTracksMock.mockResolvedValue(missing({ tracks: [track(1)] }));
    useMissingTracksStore.setState({
      artist: "Artist",
      pendingUrls: new Map([["https://example.test/track/1", "https://example.test/album/A"]]),
    });
    await useMissingTracksStore.getState().downloadMany("A", [track(1), track(2)]);
    expect(startDownloadMock).not.toHaveBeenCalled();
  });

  it("does nothing when the album has no missing data", async () => {
    getMissingTracksMock.mockResolvedValue(null);
    useMissingTracksStore.setState({ artist: "Artist" });
    await useMissingTracksStore.getState().downloadMany("A", [track(1)]);
    expect(startDownloadMock).not.toHaveBeenCalled();
  });
});

describe("no-stream songs", () => {
  const failed = (id: string, url: string, message: string) => ({ id, url, status: "failed" as const, message });
  it("recognises the yt-dlp errors of a song without a stream", () => {
    expect(isNoStreamFailure("ERROR: [Bandcamp] 1: No video formats found!")).toBe(true);
    expect(isNoStreamFailure("Network is unreachable")).toBe(false);
  });
  it("marks the only pending song of a failed album download as having no stream", () => {
    const pending = new Map([["t1", "album"]]);
    const { songs } = failedSongs(pending, { a: failed("a", "album", "No video formats found") }, new Set());
    expect(songs).toEqual([{ url: "t1", album: "album", message: "No video formats found", noStream: true }]);
  });
  it("fails every pending song but marks none as having no stream when there are several", () => {
    const two = new Map([["t1", "album"], ["t2", "album"]]);
    const { songs } = failedSongs(two, { a: failed("a", "album", "No video formats found") }, new Set());
    expect(songs.map((s) => [s.url, s.noStream])).toEqual([["t1", false], ["t2", false]]);
  });
  it("any other error is a failure of the song, not a missing stream", () => {
    const { songs } = failedSongs(new Map([["t1", "album"]]), { a: failed("a", "album", "timeout") }, new Set());
    expect(songs).toEqual([{ url: "t1", album: "album", message: "timeout", noStream: false }]);
  });
  it("ignores failed downloads that were handled before (an old failure must not fail a new download)", () => {
    const { songs, handledIds } = failedSongs(new Map([["t1", "album"]]), { a: failed("a", "album", "x") }, new Set(["a"]));
    expect(songs).toEqual([]);
    expect(handledIds).toEqual([]);
  });
  it("lowers the missing count by the loaded unavailable songs only", () => {
    const source = { album: "A", platform: "bandcamp", source_url: "u", cover_url: null, total: 3, owned: 1, missing: 2 } as const;
    const loaded = { tracks: [{ url: "t1" }, { url: "t2" }] } as never;
    expect(withoutUnavailable(source, loaded, new Set(["t1"])).missing).toBe(1);
    expect(withoutUnavailable(source, loaded, new Set(["other"])).missing).toBe(2);
    expect(withoutUnavailable(source, null, new Set(["t1"])).missing).toBe(2);
  });
});

describe("recoveredSongs", () => {
  it("forgets the failed songs of the album that are not missing any more", () => {
    const failureAlbum = { t1: "album", t2: "album", t3: "other" };
    expect(recoveredSongs(failureAlbum, "album", new Set(["t2"]))).toEqual(["t1"]);
  });
  it("keeps everything while the song is still missing", () => {
    expect(recoveredSongs({ t1: "album" }, "album", new Set(["t1"]))).toEqual([]);
  });
});
