import { beforeEach, describe, expect, it, vi } from "vitest";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";

const { handlers } = vi.hoisted(() => ({ handlers: new Map<string, (event: { payload: unknown }) => unknown>() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => unknown) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  }),
}));

beforeEach(async () => {
  handlers.clear();
  useDownloadStore.setState({
    _listenersInitialized: false,
    activeDownloads: {
      task: {
        id: "task", url: "https://example.com/album", status: "downloading",
        current: 0, total: 2, overallPercent: 0, message: "", artistName: "Artist",
        albumTitle: "Album", itemType: ItemType.Album, platform: "bandcamp",
        tracks: [
          { title: "First", percent: 0, status: "pending" },
          { title: "Second", percent: 0, status: "pending" },
        ],
      },
    },
  });
  await useDownloadStore.getState().initDownloadListeners();
});

describe("download outcomes across transfer and save events", () => {
  it("keeps transfer completion pending until saved, then preserves a partial failure", async () => {
    await handlers.get("download-track-progress")!({ payload: {
      task_id: "task", track_index: 0, track_title: "First", percent: 100,
    } });
    expect(useDownloadStore.getState().activeDownloads.task.tracks[0].status).toBe("downloading");
    await handlers.get("download-track-saved")!({ payload: {
      task_id: "task", track_index: 0, song: { title: "First" },
    } });
    await handlers.get("download-task-failed")!({ payload: {
      task_id: "task", message: "Saved 1 of 2 tracks.",
      tracks: [
        { title: "First", percent: 100, status: "completed", track_number: 1 },
        { title: "Second", percent: 100, status: "failed", track_number: 2 },
      ],
    } });
    const task = useDownloadStore.getState().activeDownloads.task;
    expect(task.status).toBe("failed");
    expect(task.message).toBe("Saved 1 of 2 tracks.");
    expect(task.tracks.map((track) => track.status)).toEqual(["completed", "failed"]);
    expect(task.tracks[1].trackNumber).toBe(2);
  });
});
