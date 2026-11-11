import { describe, it, expect, vi, beforeEach } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { AlbumPreview, AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import { useAlbumPreviewStore } from "./albumPreviewStore.ts";

type ActiveTask = { status: string; overallPercent: number };

const { getAlbumPreviewMock, setActiveViewMock, startDownloadMock, activeDownloadsMock } = vi.hoisted(
  () => ({
    getAlbumPreviewMock: vi.fn(),
    setActiveViewMock: vi.fn(),
    startDownloadMock: vi.fn(),
    // Stands in for the library's `activeDownloads`: the startDownload mock registers tasks here.
    activeDownloadsMock: {} as Record<string, ActiveTask>,
  }),
);

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/albumPreview.ts", async () => ({
  ...(await vi.importActual<typeof import("@/shared/lib/albumPreview.ts")>(
    "@/shared/lib/albumPreview.ts",
  )),
  getAlbumPreview: getAlbumPreviewMock,
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: {
    getState: () => ({
      setActiveView: setActiveViewMock,
      startDownload: startDownloadMock,
      activeDownloads: activeDownloadsMock,
    }),
  },
}));

const initialState = useAlbumPreviewStore.getState();

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
    expect(getAlbumPreviewMock).toHaveBeenCalledWith("bandcamp", ALBUM_URL);

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
    expect(startDownloadMock).toHaveBeenCalledWith({
      platform: "bandcamp",
      downloadType: "Album",
      url: ALBUM_URL,
      artistName: "Album Artist",
      title: "Album Title",
      coverUrl: COVER_URL,
      trackUrls: undefined,
    });
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
    expect(startDownloadMock).toHaveBeenCalledWith({
      platform: "bandcamp",
      downloadType: "Album",
      url: ALBUM_URL,
      artistName: "Album Artist",
      title: "Album Title",
      coverUrl: COVER_URL,
      trackUrls: ["https://example.test/track/3"],
    });
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
});
