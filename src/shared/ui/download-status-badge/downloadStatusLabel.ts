import type { DownloadState } from "@/shared/lib/libraryIndex.ts";

/** Tooltip and aria label of a library state; null when nothing is shown (`none`). Pure. */
export function downloadStatusLabel(state: DownloadState): string | null {
  switch (state) {
    case "downloaded":
      return "Downloaded";
    case "partial":
      return "Partly downloaded";
    default:
      return null;
  }
}
