import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { AlbumTrackInfo, TrackTaskState } from "@/shared/lib/remote/albumPreview.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import { AlbumTrackRow } from "@/pages/album-preview/AlbumTrackRow.tsx";
import styles from "@/pages/album-preview/album-preview.module.css";

const { activeDownloads } = vi.hoisted(() => ({
  activeDownloads: {} as Record<string, unknown>,
}));

vi.mock("@/features/preview/model/streamStore.ts", () => ({
  useStreamStore: (selector: (state: { pendingUrl: null }) => unknown) => selector({ pendingUrl: null }),
}));
vi.mock("@/shared/store/audioStore.ts", () => ({
  useAudioStore: (selector: (state: { currentSong: null; isStreaming: false; isPlaying: false; streamLoading: false }) => unknown) =>
    selector({ currentSong: null, isStreaming: false, isPlaying: false, streamLoading: false }),
}));
vi.mock("@/shared/store/downloadStore.ts", () => ({
  useDownloadStore: (selector: (state: { activeDownloads: typeof activeDownloads }) => unknown) =>
    selector({ activeDownloads }),
}));
vi.mock("@/shared/store/modalStore.ts", () => ({ useModalStore: { getState: () => ({ openModal: vi.fn() }) } }));

const candidate = {
  source: { kind: "http_stream", platform: "bandcamp", url: "https://example.com/audio" },
  quality_tier: "lossless",
  codec: "flac",
  duration_ms: 180_000,
  match_confidence: 1,
  is_strict_match: true,
  display_label: "Bandcamp",
} as AudioCandidate;

const track: AlbumTrackInfo = {
  index: 0,
  title: "Song",
  artists: ["Artist"],
  track_number: 1,
  url: "https://example.com/track",
  available: true,
  note: null,
  duration_ms: 180_000,
  candidates: [candidate],
};

const renderRow = (task: TrackTaskState | undefined, libraryState?: "downloaded") => renderToStaticMarkup(
  <AlbumTrackRow
    track={track}
    position={0}
    selected={false}
    selectedAbove={false}
    selectedBelow={false}
    libraryState={libraryState}
    task={task}
    cover={null}
    albumTitle="Album"
    onSelect={() => {}}
    onPlay={() => {}}
    onDownload={() => {}}
    onChooseQuality={() => {}}
    onContextMenu={() => {}}
  />,
);

const spanWithClass = (html: string, className: string): string | undefined => {
  const classIndex = html.indexOf(className);
  if (classIndex < 0) return undefined;
  const start = html.lastIndexOf("<span", classIndex);
  let depth = 0;
  const tags = /<span\b|<\/span>/g;
  tags.lastIndex = start;
  let match: RegExpExecArray | null;
  while ((match = tags.exec(html))) {
    depth += match[0] === "<span" ? 1 : -1;
    if (depth === 0) return html.slice(start, tags.lastIndex);
  }
  return undefined;
};

describe("AlbumTrackRow transfer controls", () => {
  it("keeps progress and cancellation together and hides source selection while running", () => {
    activeDownloads.task = {
      id: "task",
      url: track.url,
      albumTitle: "Album",
      artistName: "Artist",
      status: "downloading",
      itemType: ItemType.Track,
      current: 0,
      total: 1,
      overallPercent: 42,
      message: "",
      platform: "bandcamp",
      tracks: [{ title: "Song", percent: 42, status: "downloading" }],
    };

    const html = renderRow({ phase: "running", percent: 42, taskId: "task" });
    const slot = spanWithClass(html, styles.transferSlot);
    expect(slot).toContain("42%");
    expect(slot).toContain('aria-label="Cancel download of Song"');
    expect(html.match(/aria-label="Cancel download of Song"/g)).toHaveLength(1);
    expect(html).not.toContain(styles.candidateSelector);
  });

  it("shows the source selector for an idle downloadable track", () => {
    delete activeDownloads.task;
    const html = renderRow(undefined);
    expect(html).toContain(styles.candidateSelector);
    expect(html).toContain('aria-label="Download Song"');
  });

  it("shows downloaded status without progress or cancellation", () => {
    activeDownloads.task = {
      id: "task", url: track.url, albumTitle: "Album", artistName: "Artist", status: "downloading",
      itemType: ItemType.Track, current: 0, total: 1, overallPercent: 42, message: "", platform: "bandcamp",
      tracks: [{ title: "Song", percent: 42, status: "downloading" }],
    };
    const html = renderRow({ phase: "done" });
    expect(html).toContain("Downloaded");
    expect(html).not.toContain("42%");
    expect(html).not.toContain("Cancel download");
    expect(html).not.toContain(styles.candidateSelector);
  });

  it("does not make a running row cancellable when its task id is missing", () => {
    const html = renderRow({ phase: "running", percent: 42 });
    expect(html).toContain("42%");
    expect(html).not.toContain("Cancel download");
    expect(html).not.toContain(styles.transferCancellable);
  });
});
