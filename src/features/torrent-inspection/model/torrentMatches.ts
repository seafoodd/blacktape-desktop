import type { TorrentTrack } from "@/shared/lib/audio.ts";

export interface ExpectedTorrentTrack {
  artist?: string;
  title: string;
  disc?: number | null;
  number?: number | null;
  libraryStatus?: "owned" | "missing";
}

type TorrentMatch =
  | { kind: "matched"; file: TorrentTrack }
  | { kind: "ambiguous"; candidates: TorrentTrack[] }
  | { kind: "missing" };

const normalize = (title: string) =>
  title.normalize("NFKC").toLowerCase().replace(/[^\p{L}\p{N}\p{M}]+/gu, " ").trim();

export function mapTorrentTracks(
  expected: readonly ExpectedTorrentTrack[],
  files: readonly TorrentTrack[],
): TorrentMatch[] {
  const byTitle = new Map<string, TorrentTrack[]>();
  for (const file of files) {
    if (file.is_sample) continue;
    const title = normalize(file.title);
    if (title) byTitle.set(title, [...(byTitle.get(title) ?? []), file]);
  }
  const candidates = expected.map((track) => {
    const title = normalize(track.title);
    const artist = track.artist ? normalize(track.artist) : "";
    const filesByTitle = byTitle.get(title) ?? [];
    const filesByArtistAndTitle = artist && title ? byTitle.get(`${artist} ${title}`) ?? [] : [];
    return [...filesByTitle, ...filesByArtistAndTitle].filter((file) =>
      (track.disc == null || file.disc_number == null || track.disc === file.disc_number) &&
      (track.number == null || file.track_number == null || track.number === file.track_number),
    );
  });
  const claims = new Map<number, number>();
  for (const group of candidates) {
    for (const file of group) claims.set(file.file_index, (claims.get(file.file_index) ?? 0) + 1);
  }
  return candidates.map((group) => {
    const file = group[0];
    if (!file) return { kind: "missing" };
    if (group.length !== 1 || claims.get(file.file_index) !== 1) {
      return { kind: "ambiguous", candidates: group };
    }
    return { kind: "matched", file };
  });
}
