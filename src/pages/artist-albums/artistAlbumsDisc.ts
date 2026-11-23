import type { Song } from "@/shared/types.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { AlbumRow } from "@/features/library/ui/missing-track-row/missingTrackRow.ts";

export function albumHasMultipleDiscs(
  songs: readonly Song[],
  missing?: readonly AlbumTrackInfo[],
): boolean {
  if (songs.some((s) => (s.disc_number ?? 1) > 1)) {
    return true;
  }
  if (missing && missing.some((t) => (t.disc_number ?? 1) > 1)) {
    return true;
  }
  return false;
}

export function rowDiscNumber(row: AlbumRow): number {
  if (row.kind === "song") {
    return (row.song.disc_number && row.song.disc_number > 0) ? row.song.disc_number : 1;
  }
  return (row.track.disc_number && row.track.disc_number > 0) ? row.track.disc_number : 1;
}
