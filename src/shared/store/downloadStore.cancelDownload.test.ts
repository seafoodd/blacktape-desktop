import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useDownloadStore, type DownloadProgress } from "@/shared/store/downloadStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { buildDownloadMap } from "@/pages/search-results/searchResultsFormat.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  const task: DownloadProgress = {
    id: "album", url: "https://example.com/album", status: "downloading", current: 1, total: 2,
    overallPercent: 65, message: "Downloading", artistName: "Artist", albumTitle: "Album",
    platform: "bandcamp", itemType: ItemType.Album,
    tracks: [{ title: "Saved", percent: 100, status: "completed" }, { title: "Pending", percent: 30, status: "downloading" }],
  };
  useDownloadStore.setState({ activeDownloads: { album: task, other: { ...task, id: "other" } } });
});

describe("cancellation while downloads are active", () => {
  it("deduplicates pending requests, preserves progress on failure and permits retry", async () => {
    const task = useDownloadStore.getState().activeDownloads.album;
    const stale = { ...task, id: "old-task", status: "completed" as const };
    for (const entries of [[stale, task], [task, stale]]) {
      expect(buildDownloadMap(entries).get(task.url)?.id).toBe(task.id);
    }
    let reject!: (error: Error) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise((_, fail) => { reject = fail; }));
    const cancellation = useDownloadStore.getState().cancelDownload("album");
    expect(useDownloadStore.getState().activeDownloads.album.cancelling).toBe(true);
    await useDownloadStore.getState().cancelDownload("album");
    expect(invoke).toHaveBeenCalledOnce();
    reject(new Error("Connection lost"));
    await expect(cancellation).rejects.toThrow("Connection lost");
    const failed = useDownloadStore.getState().activeDownloads.album;
    expect(failed.cancelling).toBe(false);
    expect(failed.cancelError).toContain("Connection lost");
    expect(failed.status).toBe("downloading");
    expect(failed.tracks.map((track) => track.percent)).toEqual([100, 30]);
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await useDownloadStore.getState().cancelDownload("album");
    expect(invoke).toHaveBeenLastCalledWith("cancel_download", { taskId: "album" });
    expect(useDownloadStore.getState().activeDownloads.album).toBeUndefined();
    expect(useDownloadStore.getState().activeDownloads.other).toBeDefined();
  });

  it("does not resurrect a task removed by a backend event while cancellation is pending", async () => {
    let finish!: () => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise<void>((resolve) => { finish = resolve; }));
    const pending = useDownloadStore.getState().cancelDownload("album");
    useDownloadStore.setState((state) => ({ activeDownloads: { other: state.activeDownloads.other } }));
    finish();
    await pending;
    expect(Object.keys(useDownloadStore.getState().activeDownloads)).toEqual(["other"]);
  });
});
