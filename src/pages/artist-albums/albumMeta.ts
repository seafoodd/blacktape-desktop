import { Song } from "@/shared/types.ts";

/** Release year of an album: "2014", or "2012–2014" when its tracks differ. */
export function albumYearLabel(songs: Pick<Song, "release_year">[]): string | null {
  const years = songs
    .map((s) => s.release_year)
    .filter((y): y is number => typeof y === "number" && y > 0);
  if (years.length === 0) return null;
  const min = Math.min(...years);
  const max = Math.max(...years);
  return min === max ? String(min) : `${min}–${max}`;
}

/**
 * Distinct genres of an album's tracks (a genre tag can hold several, separated by
 * ";", "/" or ","). Compared case-insensitively, shown as first written, in order of
 * appearance.
 */
export function albumGenres(songs: Pick<Song, "genre">[]): string[] {
  const seen = new Set<string>();
  const genres: string[] = [];
  for (const song of songs) {
    for (const raw of (song.genre ?? "").split(/[;/,]/)) {
      const genre = raw.trim();
      const key = genre.toLowerCase();
      if (!genre || seen.has(key)) continue;
      seen.add(key);
      genres.push(genre);
    }
  }
  return genres;
}

/** "2014 · Electronic, IDM" for the line under an album title, or null if empty. */
export function albumMetaLine(
  songs: Pick<Song, "release_year" | "genre">[],
  maxGenres = 4,
): string | null {
  const year = albumYearLabel(songs);
  const genres = albumGenres(songs).slice(0, maxGenres);
  const parts = [year, genres.length > 0 ? genres.join(", ") : null].filter(
    (p): p is string => p !== null,
  );
  return parts.length > 0 ? parts.join(" · ") : null;
}
