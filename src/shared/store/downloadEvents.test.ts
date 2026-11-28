import { describe, expect, it } from "vitest";
import { reduceDownloadEvent, type DownloadEvent } from "@/shared/store/downloadEvents.ts";
import type { DownloadProgress } from "@/shared/store/downloadStore.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import type { Song } from "@/shared/types.ts";

describe("download event state transitions", () => {
  it("preserves committed tracks through delayed progress, metadata, failure and cancellation", () => {
    const initial: DownloadProgress = {
      id: "task", url: "https://example.com/album", status: "downloading", current: 0,
      total: 0, message: "", artistName: "Artist", albumTitle: "Album", itemType: ItemType.Album,
      platform: "bandcamp", overallPercent: 0, tracks: [],
    };
    const unrelated = { ...initial, id: "unrelated" };
    let active: Record<string, DownloadProgress> = { task: initial, unrelated };
    const apply = (event: DownloadEvent) => {
      active = reduceDownloadEvent(active, event);
    };
    const metadata: DownloadEvent = {
      type: "metadata", task_id: "task", title: "Album", artist: "Artist",
      tracks: [{ title: "First", track_number: 1 }, { title: "Second", track_number: 2 }],
    };
    apply(metadata);
    apply({ type: "track-progress", task_id: "task", track_index: 0, track_title: "First", percent: 150 });
    expect(active.task.tracks[0]).toMatchObject({ percent: 100, status: "downloading" });
    apply({ type: "saved", task_id: "task", track_index: 0, song: { title: "First" } as Song });
    apply({ type: "track-progress", task_id: "task", track_index: 0, track_title: "First", percent: 20 });
    apply(metadata);
    expect(active.task.tracks[0]).toMatchObject({ percent: 100, status: "completed" });
    apply({ type: "track-progress", task_id: "task", track_index: 1, track_title: "Second", percent: -5 });
    expect(active.task.tracks[1].percent).toBe(0);
    apply({ type: "failed", task_id: "task", message: "Saved 1 of 2 tracks.", tracks: [
      { title: "First", percent: 100, status: "completed", track_number: 1 },
      { title: "Second", percent: 0, status: "failed", track_number: 2 },
    ] });
    const failed = active;
    apply({ type: "progress", task_id: "task", current: 0, total: 2, track_title: "Old progress" });
    expect(active).toBe(failed);
    expect(active.task.status).toBe("failed");
    expect(active.task.tracks.map((track) => track.status)).toEqual(["completed", "failed"]);
    expect(active.unrelated).toBe(unrelated);
    apply({ type: "cancelled", task_id: "task" });
    expect(active.task).toBeUndefined();
    expect(active.unrelated).toBe(failed.unrelated);
  });

  it("treats track-cancelled as terminal and ignores late progress or saved events", () => {
    const initial: DownloadProgress = {
      id: "task", url: "https://example.com/album", status: "downloading", current: 0,
      total: 2, message: "", artistName: "Artist", albumTitle: "Album", itemType: ItemType.Album,
      platform: "bandcamp", overallPercent: 0,
      tracks: [
        { title: "A", percent: 50, status: "downloading", trackNumber: 1 },
        { title: "B", percent: 40, status: "downloading", trackNumber: 2 },
      ],
    };
    let active: Record<string, DownloadProgress> = { task: initial };
    active = reduceDownloadEvent(active, { type: "track-cancelled", task_id: "task", track_index: 1 });
    expect(active.task.tracks[1]).toMatchObject({ status: "cancelled", percent: 0 });
    active = reduceDownloadEvent(active, { type: "track-progress", task_id: "task", track_index: 1, track_title: "B", percent: 90 });
    expect(active.task.tracks[1]).toMatchObject({ status: "cancelled", percent: 0 });
    active = reduceDownloadEvent(active, { type: "saved", task_id: "task", track_index: 1, song: { title: "B" } as Song });
    expect(active.task.tracks[1]).toMatchObject({ status: "cancelled", percent: 0 });
    expect(active.task.tracks[0]).toMatchObject({ status: "downloading", percent: 50 });
  });
});
