import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DownloadProgress } from "@/shared/store/downloadStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { PlayerDownloadControl } from "@/features/player/ui/PlayerDownloadControl.tsx";

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

const activeTask = (overrides: Partial<DownloadProgress> = {}): DownloadProgress => ({
  id: "task-1",
  url: "https://example.com/album",
  albumTitle: "Album",
  artistName: "Artist",
  status: "downloading",
  itemType: ItemType.Album,
  platform: "bandcamp",
  current: 0,
  total: 2,
  overallPercent: 37,
  message: "",
  tracks: [],
  ...overrides,
});

const renderControl = (percent = 37) => renderToStaticMarkup(
  <PlayerDownloadControl taskId="task-1" percent={percent} />,
);

describe("PlayerDownloadControl", () => {
  beforeEach(() => {
    for (const key of Object.keys(tasks)) delete tasks[key];
    cancel.mockClear();
    openModal.mockClear();
  });

  it("shows accessible progress and a cancel action without exposing the percentage as visible control text", () => {
    tasks["task-1"] = activeTask();

    const html = renderControl();

    expect(html).toMatch(/<span[^>]*class="[^"]*downloadStatus[^"]*"[^>]*role="status"[^>]*>Downloading 37%<\/span>/);
    expect(html).toMatch(/<span[^>]*class="[^"]*downloadSpinner[^"]*"[^>]*aria-hidden="true"[^>]*><\/span>/);
    expect(html).toMatch(/<button[^>]*aria-label="Cancel download of Album"[^>]*>/);
    expect(html).not.toMatch(/<button[^>]*>[^<]*37%/);
    expect((html.match(/<button\b/g) ?? [])).toHaveLength(1);
  });

  it("keeps the cancel control disabled and busy while cancellation is pending", () => {
    tasks["task-1"] = activeTask({ cancelling: true });

    const html = renderControl();

    expect(html).toMatch(/<button[^>]*aria-label="Cancelling download…"[^>]*aria-busy="true"[^>]*disabled=""[^>]*>/);
    expect(html).toContain("Downloading 37%");
  });

  it("does not render an actionable cancel control after the task completes", () => {
    tasks["task-1"] = activeTask({ status: "completed" });

    const html = renderControl(100);

    expect(html).toContain("Downloading 100%");
    expect(html).not.toContain("<button");
  });
});
