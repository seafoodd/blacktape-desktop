export function lyricsEmptyText(
  hasCurrentSong: boolean,
  errorStatus: string | null,
): string {
  if (!hasCurrentSong) {
    return "Play a track to view lyrics";
  }
  return errorStatus || "No lyrics found.";
}
