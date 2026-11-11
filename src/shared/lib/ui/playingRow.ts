/**
 * Which row of a list is the one currently playing.
 *
 * A song can appear several times in a playlist or queue, so matching by id would
 * highlight every copy. `preferredIndex` is the position the player reports; it is
 * trusted when that row still holds the playing song, otherwise the first row with
 * the song's id is used (e.g. the list was edited since playback started).
 * Returns null when nothing in the list is playing.
 */
export function findPlayingRow(
  songs: { id: number }[],
  currentId: number | null | undefined,
  preferredIndex: number | null | undefined,
): number | null {
  if (currentId === null || currentId === undefined) return null;
  if (
    preferredIndex !== null &&
    preferredIndex !== undefined &&
    songs[preferredIndex]?.id === currentId
  ) {
    return preferredIndex;
  }
  const first = songs.findIndex((song) => song.id === currentId);
  return first === -1 ? null : first;
}
