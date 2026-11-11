import { invoke } from "@tauri-apps/api/core";

/** Tauri event emitted (payload empty) after every trash mutation. */
export const TRASH_CHANGED_EVENT = "trash-changed";

export type TrashItem = {
  id: number;
  title: string;
  artists: string[];
  album: string;
  /** Cover image path or URL, if the song had one. */
  cover_url?: string | null;
  original_path: string;
  size_bytes: number;
  /** Unix seconds. */
  trashed_at: number;
};

/** null = never delete by age / unlimited size. */
export type TrashSettings = {
  retention_days: number | null;
  max_size_mb: number | null;
};

export type TrashOutcome = {
  done: number;
  failures: string[];
};

/** Moves songs (by library song id) to the trash. */
export const trashSongs = (ids: number[]): Promise<TrashOutcome> =>
  invoke<TrashOutcome>("trash_songs", { ids });

/** Newest first. */
export const listTrash = (): Promise<TrashItem[]> =>
  invoke<TrashItem[]>("list_trash");

export const restoreTrash = (ids: number[]): Promise<TrashOutcome> =>
  invoke<TrashOutcome>("restore_trash", { ids });

/** Permanent. */
export const deleteTrash = (ids: number[]): Promise<TrashOutcome> =>
  invoke<TrashOutcome>("delete_trash", { ids });

/** Permanent, all items. */
export const emptyTrash = (): Promise<TrashOutcome> =>
  invoke<TrashOutcome>("empty_trash");

export const getTrashSettings = (): Promise<TrashSettings> =>
  invoke<TrashSettings>("get_trash_settings");

export const setTrashSettings = (
  retentionDays: number | null,
  maxSizeMb: number | null,
): Promise<void> =>
  invoke<void>("set_trash_settings", { retentionDays, maxSizeMb });

// ---------------------------------------------------------------------------
// Pure helpers (formatting and settings conversion). Kept free of React and
// Tauri state so they can be unit tested.
// ---------------------------------------------------------------------------

const SECONDS_PER_DAY = 24 * 60 * 60;
const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** Binary units (1 GB = 1024 MB), matching the size limit conversion below. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < SIZE_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  if (unit === 0) return `${Math.round(value)} B`;
  // Rounding can reach 1024 (e.g. 1023.9 KB); show it in the next unit instead.
  if (Math.round(value) >= 1024 && unit < SIZE_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = value < 10 ? 1 : 0;
  return `${value.toFixed(digits)} ${SIZE_UNITS[unit]}`;
}

/** Whole days left before an item is auto-deleted, rounded up; never negative. */
export function trashDaysLeft(
  trashedAtSecs: number,
  retentionDays: number | null,
  nowSecs: number,
): number | null {
  if (retentionDays === null) return null;
  const deleteAtSecs = trashedAtSecs + retentionDays * SECONDS_PER_DAY;
  const remainingSecs = deleteAtSecs - nowSecs;
  if (remainingSecs <= 0) return 0;
  return Math.ceil(remainingSecs / SECONDS_PER_DAY);
}

export function formatDaysLeft(days: number): string {
  if (days <= 0) return "Deletes today";
  if (days === 1) return "Deletes in 1 day";
  return `Deletes in ${days} days`;
}

/** Unix seconds to a short local date, e.g. "Nov 8, 2026". */
export function formatTrashedDate(trashedAtSecs: number): string {
  return new Date(trashedAtSecs * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** One-line summary of the retention and size limit, for the Trash page. */
export function describeTrashPolicy(settings: TrashSettings): string {
  const age =
    settings.retention_days === null
      ? "Items are kept until you delete them."
      : `Items are deleted automatically after ${settings.retention_days} ${settings.retention_days === 1 ? "day" : "days"}.`;
  if (settings.max_size_mb === null) return age;
  return `${age} The Trash is limited to ${formatBytes(settings.max_size_mb * 1024 * 1024)}.`;
}

export const gbToMb = (gb: number): number => gb * 1024;

export const mbToGb = (mb: number): number => mb / 1024;

/** Retention choices for the Settings select. null = never delete by age. */
export const RETENTION_OPTIONS: { label: string; days: number | null }[] = [
  { label: "Never", days: null },
  { label: "1 day", days: 1 },
  { label: "7 days", days: 7 },
  { label: "14 days", days: 14 },
  { label: "30 days", days: 30 },
  { label: "60 days", days: 60 },
  { label: "90 days", days: 90 },
];

/** Size limit choices for the Settings select. null = unlimited. */
export const MAX_SIZE_OPTIONS: { label: string; gb: number | null }[] = [
  { label: "Unlimited", gb: null },
  { label: "1 GB", gb: 1 },
  { label: "2 GB", gb: 2 },
  { label: "5 GB", gb: 5 },
  { label: "10 GB", gb: 10 },
  { label: "20 GB", gb: 20 },
  { label: "50 GB", gb: 50 },
];
