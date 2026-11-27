import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { DownloadProgress } from "@/shared/store/downloadStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { TrackDownloadProgress } from "@/shared/ui/track-download-progress/TrackDownloadProgress.tsx";
import { CancelDownloadButton } from "@/shared/ui/cancel-download-button/CancelDownloadButton.tsx";

const { tasks, openModal } = vi.hoisted(() => ({
  tasks: {} as Record<string, DownloadProgress>,
  openModal: vi.fn(),
}));

vi.mock("@/shared/store/downloadStore.ts", () => ({
  useDownloadStore: Object.assign(
    (selector: (state: { activeDownloads: typeof tasks }) => unknown) => selector({ activeDownloads: tasks }),
    { getState: () => ({ cancelDownload: vi.fn() }) },
  ),
}));
vi.mock("@/shared/store/modalStore.ts", () => ({
  useModalStore: Object.assign(
    (selector: (state: { closeModal: () => void }) => unknown) => selector({ closeModal: vi.fn() }),
    { getState: () => ({ openModal }) },
  ),
}));

function task(overrides: Partial<DownloadProgress> = {}): DownloadProgress {
  return {
    id: "task-1", url: "https://example.test/album", status: "downloading", current: 1, total: 4,
    message: "Downloading", artistName: "Artist", albumTitle: "Album", itemType: ItemType.Album,
    platform: "bandcamp", overallPercent: 25, tracks: [], ...overrides,
  };
}

describe("TrackDownloadProgress", () => {
  beforeEach(() => {
    for (const key of Object.keys(tasks)) delete tasks[key];
    openModal.mockClear();
  });

  it("shows progress and one cancel action for an active task", () => {
    tasks["task-1"] = task();
    const html = renderToStaticMarkup(<TrackDownloadProgress taskId="task-1" percent={37} />);
    expect(html).toContain("37%");
    expect(html).toContain("cancellable");
    expect(html.match(/<button/g)).toHaveLength(1);
    expect(html).toContain('aria-label="Cancel download of Album"');
  });

  it.each([
    ["finished prop", task(), true],
    ["completed task", task({ status: "completed" }), false],
    ["failed task", task({ status: "failed" }), false],
  ])("keeps progress without a cancel action for %s", (_label, active, finished) => {
    tasks["task-1"] = active as DownloadProgress;
    const html = renderToStaticMarkup(<TrackDownloadProgress taskId="task-1" percent={100} finished={finished as boolean} />);
    expect(html).toContain("100%");
    expect(html).not.toContain("<button");
  });

  it("keeps progress when no task id was supplied", () => {
    const html = renderToStaticMarkup(<TrackDownloadProgress percent={12} />);
    expect(html).toContain("12%");
    expect(html).not.toContain("<button");
  });

  it("disables cancellation while the task is already cancelling", () => {
    tasks["task-1"] = task({ cancelling: true });
    const html = renderToStaticMarkup(<TrackDownloadProgress taskId="task-1" percent={37} />);
    expect(html).toContain("37%");
    expect(html).toContain('disabled=""');
    expect(html).toContain('aria-busy="true"');
  });

  it("opens a confirmation that says an album task is cancelled in full", () => {
    tasks["task-1"] = task();
    const cancel = CancelDownloadButton({ taskId: "task-1" });
    if (!cancel) throw new Error("Active album task should have a cancel action");
    cancel.props.onClick({ stopPropagation: vi.fn() });
    const modal = openModal.mock.calls[0][0];
    const modalHtml = renderToStaticMarkup(modal);
    expect(modalHtml).toContain("This stops the entire download task.");
  });
});
