/** Payload of dragging one or more songs (e.g. onto a playlist in the sidebar). */

export const SONG_DRAG_MIME = "application/json";

export function encodeSongDrag(songIds: number[]): string {
  // `songId` is kept for consumers that only know the single-song payload.
  return JSON.stringify({ songId: songIds[0], songIds });
}

/** Song ids from a drag payload, or [] if it is missing or malformed. */
export function decodeSongDrag(data: string): number[] {
  try {
    const parsed = JSON.parse(data);
    if (Array.isArray(parsed?.songIds)) {
      return parsed.songIds.filter(
        (id: unknown): id is number => typeof id === "number",
      );
    }
    if (typeof parsed?.songId === "number") return [parsed.songId];
  } catch (_) {}
  return [];
}
