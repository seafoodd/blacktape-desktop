import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useLibraryStore, ItemType } from "@/shared/store/libraryStore.ts";

const registeredListeners: Record<string, (event: any) => Promise<void> | void> = {};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_active_downloads") return [];
    if (cmd === "get_artists") return [];
    if (cmd === "get_artist_albums") return [];
    return null;
  }),
  convertFileSrc: (p: string) => p,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (e: any) => Promise<void> | void) => {
    registeredListeners[event] = handler;
    return () => {};
  }),
}));

describe("libraryStore download-track-saved listener", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useLibraryStore.setState({
      activeDownloads: {
        "task-1": {
          id: "task-1",
          url: "https://example.com/album",
          artistName: "Test Artist",
          albumTitle: "Test Album",
          status: "downloading",
          current: 0,
          total: 2,
          overallPercent: 0,
          message: "",
          itemType: ItemType.Album,
          platform: "youtube" as any,
          tracks: [
            { title: "Track 1", trackNumber: 1, percent: 50, status: "downloading" },
            { title: "Track 2", trackNumber: 2, percent: 0, status: "pending" },
          ],
        },
      },
      selectedTab: "Test Artist",
      _listenersInitialized: false,
    });
  });

  it("updates track to completed and triggers swap and refresh when download-track-saved arrives", async () => {
    const { initDownloadListeners } = useLibraryStore.getState();
    await initDownloadListeners();

    expect(registeredListeners["download-track-saved"]).toBeDefined();

    const mockSong = {
      title: "Track 1",
      album_artist: "Test Artist",
      album: "Test Album",
      source_url: "https://example.com/track1",
    };

    registeredListeners["download-track-saved"]({
      payload: {
        task_id: "task-1",
        track_index: 0,
        song: mockSong,
      },
    });

    const active = useLibraryStore.getState().activeDownloads["task-1"];
    expect(active.tracks[0].percent).toBe(100);
    expect(active.tracks[0].status).toBe("completed");

    expect(invoke).toHaveBeenCalledWith("swap_streams_for_downloads", {
      songs: [mockSong],
    });
  });
});
