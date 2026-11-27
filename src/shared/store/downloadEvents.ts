import type { DownloadProgress, TrackDownloadProgress } from "@/shared/store/downloadStore.ts";
import type { Song } from "@/shared/types.ts";

interface Outcome {
  task_id: string;
  message: string;
  tracks?: {
    title: string;
    percent: number;
    status: TrackDownloadProgress["status"];
    track_number?: number;
    disc_number?: number;
  }[];
}

export type DownloadEvent =
  | ({ type: "started" } & Outcome)
  | ({ type: "completed" | "failed" } & Outcome)
  | { type: "cancelled"; task_id: string }
  | {
    type: "metadata";
    task_id: string;
    title: string;
    artist: string;
    cover_url?: string;
    release_year?: number;
    tracks: { title: string; track_number?: number; disc_number?: number }[];
  }
  | { type: "track-progress"; task_id: string; track_index: number; track_title: string; percent: number }
  | { type: "saved"; task_id: string; track_index: number; song: Song }
  | { type: "progress"; task_id: string; current: number; total: number; track_title: string };

/** Rust outcomes are final. Buffered transfer progress can only advance an active task. */
export function reduceDownloadEvent(
  active: Record<string, DownloadProgress>,
  event: DownloadEvent,
): Record<string, DownloadProgress> {
  const task = active[event.task_id];
  if (!task) return active;
  if (event.type === "cancelled") {
    const next = { ...active };
    delete next[event.task_id];
    return next;
  }
  const terminal = task.status === "completed" || task.status === "failed";
  if (terminal && event.type !== "completed" && event.type !== "failed") return active;

  let updated: DownloadProgress;
  switch (event.type) {
    case "started":
      updated = { ...task, status: "downloading", message: event.message };
      break;
    case "metadata":
      updated = {
        ...task,
        artistName: event.artist || task.artistName,
        albumTitle: event.title || task.albumTitle,
        coverUrl: event.cover_url || task.coverUrl,
        releaseYear: event.release_year ?? task.releaseYear,
        total: event.tracks.length,
        tracks: event.tracks.map((track, index) => ({
          title: track.title,
          trackNumber: track.track_number ?? index + 1,
          discNumber: track.disc_number,
          percent: task.tracks[index]?.percent ?? 0,
          status: task.tracks[index]?.status ?? "pending",
        })),
      };
      break;
    case "track-progress":
    case "saved": {
      const tracks = [...task.tracks];
      const title = event.type === "saved" ? event.song.title : event.track_title;
      while (tracks.length <= event.track_index) {
        tracks.push({
          title: title || `Track ${tracks.length + 1}`,
          percent: 0,
          status: "pending",
          trackNumber: tracks.length + 1,
        });
      }
      const previous = tracks[event.track_index];
      const saved = event.type === "saved" || previous.status === "completed";
      const percent = event.type === "track-progress" ? event.percent : 100;
      tracks[event.track_index] = {
        ...previous,
        title: title || previous.title,
        percent: saved ? 100 : Math.max(previous.percent, Math.min(100, Math.max(0, percent))),
        status: saved ? "completed" : "downloading",
      };
      updated = {
        ...task,
        tracks,
        overallPercent: Math.round(tracks.reduce((sum, track) => sum + track.percent, 0) / tracks.length),
      };
      break;
    }
    case "progress":
      updated = {
        ...task,
        status: "downloading",
        current: Math.max(task.current, event.current),
        total: event.total > 0 ? event.total : task.total,
        message: event.track_title,
        overallPercent: task.tracks.length > 0 || event.total <= 0
          ? task.overallPercent
          : Math.max(task.overallPercent, Math.round(event.current / event.total * 100)),
      };
      break;
    case "completed":
    case "failed":
      updated = {
        ...task,
        status: event.type,
        message: event.message,
        overallPercent: event.type === "completed" ? 100 : task.overallPercent,
        tracks: event.tracks?.map((track) => ({
          title: track.title,
          percent: track.percent,
          status: track.status,
          trackNumber: track.track_number,
          discNumber: track.disc_number,
        })) ?? task.tracks,
      };
      break;
  }
  return { ...active, [event.task_id]: updated };
}
