import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useLibraryStore, ItemType } from "@/shared/store/libraryStore.ts";

vi.mock("@tauri-apps/api/core", () => ({
  // Never resolves: the download stays "downloading" for the whole test.
  invoke: vi.fn(() => new Promise(() => {})),
  convertFileSrc: (p: string) => p,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const ALBUM = "https://a.bandcamp.com/album/x";
const TRACK_1 = "https://a.bandcamp.com/track/t";
const TRACK_2 = "https://a.bandcamp.com/track/u";

function albumParams(trackUrls: string[]) {
  return {
    platform: "bandcamp",
    downloadType: "Album",
    url: ALBUM,
    trackUrls,
  };
}

// startDownload resolves only when invoke does, which never happens here; its guard and state update run synchronously,
// so the calls are fire-and-forget.
beforeEach(() => {
  vi.clearAllMocks();
  useLibraryStore.setState({ activeDownloads: {} });
});

describe("startDownload guard", () => {
  it("does not start a second download of the same album tracks while the first runs", () => {
    const { startDownload } = useLibraryStore.getState();

    void startDownload(albumParams([TRACK_1]));
    void startDownload(albumParams([TRACK_1]));

    const entries = Object.values(useLibraryStore.getState().activeDownloads);
    expect(entries).toHaveLength(1);
    expect(entries[0].trackUrls).toEqual([TRACK_1]);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("adds a second entry for a different track of the same album", () => {
    const { startDownload } = useLibraryStore.getState();

    void startDownload(albumParams([TRACK_1]));
    void startDownload(albumParams([TRACK_2]));

    const entries = Object.values(useLibraryStore.getState().activeDownloads);
    expect(entries).toHaveLength(2);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("starts a new download once the matching entry has failed", () => {
    const { startDownload } = useLibraryStore.getState();

    void startDownload(albumParams([TRACK_1]));
    const [firstId] = Object.keys(useLibraryStore.getState().activeDownloads);
    useLibraryStore.setState((state) => ({
      activeDownloads: {
        ...state.activeDownloads,
        [firstId]: { ...state.activeDownloads[firstId], status: "failed" },
      },
    }));

    void startDownload(albumParams([TRACK_1]));

    expect(Object.keys(useLibraryStore.getState().activeDownloads)).toHaveLength(2);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("does not add a second entry for a single track downloaded twice", () => {
    const { startDownload } = useLibraryStore.getState();
    const trackParams = {
      platform: "bandcamp",
      downloadType: ItemType.Track,
      url: TRACK_1,
    };

    void startDownload(trackParams);
    void startDownload(trackParams);

    expect(Object.keys(useLibraryStore.getState().activeDownloads)).toHaveLength(1);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
