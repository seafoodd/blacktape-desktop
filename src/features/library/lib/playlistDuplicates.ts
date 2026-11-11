/** Splits songs about to be added to a playlist into new ones and ones it already has. */
export function splitDuplicates(
  existingIds: number[],
  newIds: number[],
): { fresh: number[]; duplicates: number[] } {
  const existing = new Set(existingIds);
  const fresh: number[] = [];
  const duplicates: number[] = [];
  for (const id of newIds) {
    (existing.has(id) ? duplicates : fresh).push(id);
  }
  return { fresh, duplicates };
}
