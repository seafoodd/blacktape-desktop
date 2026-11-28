import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";

const { handlers, stop } = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => unknown>(),
  stop: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const snapshot = [{
  id: "task", platform: "bandcamp", download_type: "Album", url: "https://example.com/album",
  artist_name: "Artist", album_title: "Album", status: "downloading", current: 0, total: 1,
  overall_percent: 80, message: "Downloading",
  tracks: [{ title: "Song", track_number: 1, percent: 80, status: "downloading" }],
}];

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  handlers.clear();
  vi.mocked(listen).mockImplementation(async (name, handler) => {
    handlers.set(name, handler as (event: { payload: unknown }) => unknown);
    return stop;
  });
  vi.mocked(invoke).mockResolvedValue([]);
  useDownloadStore.setState({ _listenersInitialized: false, activeDownloads: {} });
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe("download recovery while events arrive", () => {
  it.each(["completed", "failed", "cancelled"])("replays %s after a stale snapshot and rejects delayed progress", async (outcome) => {
    let release!: (value: typeof snapshot) => void;
    const pending = new Promise<typeof snapshot>((resolve) => { release = resolve; });
    vi.mocked(invoke).mockImplementation((command) => command === "get_active_downloads" ? pending : Promise.resolve([]));
    const initializing = useDownloadStore.getState().initDownloadListeners();
    // The snapshot must be requested only after every listener is attached.
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("get_active_downloads"));
    expect(handlers.size).toBe(9);
    handlers.get(`download-task-${outcome}`)!({ payload: {
      task_id: "task", message: outcome,
      tracks: [{ title: "Song", percent: 100, status: outcome === "completed" ? "completed" : "failed" }],
    } });
    handlers.get("download-task-progress")!({ payload: { task_id: "task", current: 0, total: 1, track_title: "Old progress" } });
    release(snapshot);
    await initializing;
    const task = useDownloadStore.getState().activeDownloads.task;
    if (outcome === "cancelled") {
      expect(task).toBeUndefined();
    } else {
      expect(task.status).toBe(outcome);
      expect(task.message).toBe(outcome);
      expect(task.tracks[0].status).toBe(outcome);
    }
  });

  it("keeps newer snapshot progress when an older buffered event is replayed", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command !== "get_active_downloads") return [];
      handlers.get("download-track-progress")!({ payload: { task_id: "task", track_index: 0, percent: 20 } });
      return snapshot;
    });
    await useDownloadStore.getState().initDownloadListeners();
    expect(useDownloadStore.getState().activeDownloads.task.tracks[0].percent).toBe(80);
  });

  it("cleans up partial subscriptions and allows recovery to retry after registration fails", async () => {
    vi.mocked(listen).mockImplementationOnce(async () => stop).mockRejectedValueOnce(new Error("Listener unavailable"));
    await expect(useDownloadStore.getState().initDownloadListeners()).rejects.toThrow("Listener unavailable");
    expect(stop).toHaveBeenCalledOnce();
    expect(useDownloadStore.getState()._listenersInitialized).toBe(false);
    await useDownloadStore.getState().initDownloadListeners();
    expect(invoke).toHaveBeenCalledWith("get_active_downloads");
  });
});
