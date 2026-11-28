import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import type { DownloadProgress } from "@/shared/store/downloadStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { CancelDownloadButton } from "@/shared/ui/cancel-download-button/CancelDownloadButton.tsx";

const { tasks, cancel, openModal } = vi.hoisted(() => ({
  tasks: {} as Record<string, DownloadProgress>,
  cancel: vi.fn(async () => {}),
  openModal: vi.fn(),
}));

vi.mock("@/shared/store/downloadStore.ts", () => ({
  useDownloadStore: Object.assign(
    (selector: (state: { activeDownloads: typeof tasks }) => unknown) => selector({ activeDownloads: tasks }),
    { getState: () => ({ cancelDownload: cancel }) },
  ),
}));
vi.mock("@/shared/store/modalStore.ts", () => ({ useModalStore: { getState: () => ({ openModal }) } }));

describe("cancel control task selection and interaction", () => {
  it("groups active tasks once, excludes stale tasks, stops row actions and reflects pending cancellation", async () => {
    const album: DownloadProgress = {
      id: "album", url: "https://example.com/album", albumTitle: "Album", artistName: "Artist",
      status: "downloading", itemType: ItemType.Album, platform: "bandcamp", current: 0,
      total: 2, overallPercent: 20, message: "", tracks: [],
    };
    Object.assign(tasks, {
      album,
      upgrade: { ...album, id: "upgrade", itemType: ItemType.Track, tracks: [{ title: "Song", percent: 10, status: "downloading" }] },
      finished: { ...album, id: "finished", status: "completed" },
    });
    const button = CancelDownloadButton({ taskIds: ["album", "upgrade", "album", "finished", "missing"] });
    if (!button) throw new Error("Active downloads should have a cancel control");
    expect(button.props["aria-label"]).toBe("Cancel downloads for Album");
    const event = { stopPropagation: vi.fn(), preventDefault: vi.fn() };
    button.props.onMouseDown(event);
    button.props.onClick(event);
    expect(event.stopPropagation).toHaveBeenCalledTimes(2);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    const modal = openModal.mock.calls[0][0] as ReactElement<{ taskCount: number; onConfirm: () => Promise<void> }>;
    expect(modal.props.taskCount).toBe(2);
    expect(cancel).not.toHaveBeenCalled();
    await modal.props.onConfirm();
    expect(cancel.mock.calls).toEqual([["album"], ["upgrade"]]);
    tasks.upgrade.cancelling = true;
    expect(CancelDownloadButton({ taskIds: ["album", "upgrade"] })?.props.disabled).toBe(true);
    expect(CancelDownloadButton({ taskId: "upgrade" })?.props["aria-label"]).toBe("Cancelling download…");
    expect(CancelDownloadButton({ taskIds: ["finished", "missing"] })).toBeNull();
    tasks.upgrade.cancelling = false;
    expect(CancelDownloadButton({ taskId: "upgrade" })?.props["aria-label"]).toBe("Cancel download of Song");
  });
});
