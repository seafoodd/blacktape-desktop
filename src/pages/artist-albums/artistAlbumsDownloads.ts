import type { Album, Song } from "@/shared/types.ts";
import type { LibraryAlbumSource } from "@/shared/lib/library/libraryIndex.ts";
import { sameUrl } from "@/shared/lib/remote/urlKey.ts";
import type { DownloadProgress } from "@/shared/store/libraryStore.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";

const ACTIVE_STATUSES = new Set(["downloading", "idle", "processing"]);

export function groupMissingSelection<T extends { album: string; track: { url: string } }>(
  tracks: readonly T[],
  canDownload: (url: string) => boolean,
): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  for (const item of tracks) {
    if (!canDownload(item.track.url)) continue;
    const group = groups.get(item.album);
    if (group) group.push(item);
    else groups.set(item.album, [item]);
  }
  return groups;
}

export function isActiveLibraryDownload(d: Pick<DownloadProgress, "status" | "url">): boolean {
  return ACTIVE_STATUSES.has(d.status) && !d.url.startsWith("resolve:");
}

/** A destructive action needs a unique source or title/disc match; track numbers alone are insufficient. */
export function findMissingTrackDownload(
  downloads: readonly DownloadProgress[],
  track: AlbumTrackInfo,
): DownloadProgress | undefined {
  const title = normalizeAlbumTitle(track.title);
  const disc = track.disc_number ?? 1;
  const metadataScore = (candidateTitle: string, candidateDisc?: number | null, candidateNumber?: number | null): number => {
    if (!title || normalizeAlbumTitle(candidateTitle) !== title) return 0;
    if (candidateDisc != null && candidateDisc !== disc) return 0;
    if (candidateNumber != null && track.track_number != null && candidateNumber !== track.track_number) return 0;
    return candidateDisc != null ? 2 : 1;
  };
  let bestScore = 0;
  let matches: DownloadProgress[] = [];
  for (const download of downloads) {
    if (!isActiveLibraryDownload(download)) continue;
    const progress = download.placement || download.itemType === "Track"
      ? download.tracks.slice(0, 1)
      : download.tracks.filter((item) => metadataScore(item.title, item.discNumber, item.trackNumber) > 0);
    if (progress.length > 0 && progress.every((item) => item.status === "completed" || item.status === "failed")) continue;
    let score: number;
    if (download.trackUrls?.length) {
      score = track.url && download.trackUrls.some((url) => sameUrl(url, track.url)) ? 3 : 0;
    } else if (track.url && sameUrl(download.url, track.url)) {
      score = 3;
    } else if (download.placement) {
      score = metadataScore(download.placement.title, download.placement.disc_number, download.placement.track_number);
    } else {
      score = download.tracks.reduce((best, item) => Math.max(best, metadataScore(item.title, item.discNumber, item.trackNumber)), 0);
    }
    if (score > bestScore) {
      bestScore = score;
      matches = [download];
    } else if (score > 0 && score === bestScore) {
      matches.push(download);
    }
  }
  return matches.length === 1 ? matches[0] : undefined;
}

export function normalizeAlbumTitle(title: string): string {
  return title.trim().toLowerCase();
}

export async function prepareAlbumDeletion(
  album: Pick<Album, "title"> & { songs: readonly Pick<Song, "id">[] },
  artist: string,
  downloads: readonly DownloadProgress[],
  linkedSource: string | undefined,
  cancel: (id: string) => Promise<void>,
  fetchAlbums: (artist: string) => Promise<Album[]>,
): Promise<Song[]> {
  const songIds = new Set(album.songs.map((song) => song.id));
  const matching = downloads.filter((task) => {
    if (!ACTIVE_STATUSES.has(task.status)) return false;
    if (task.replaceSongId !== undefined && songIds.has(task.replaceSongId)) return true;
    if ([task.url, ...(task.trackUrls ?? [])].some((url) => {
      const match = /^resolve:song:(\d+)$/.exec(url);
      return match && songIds.has(Number(match[1]));
    })) return true;
    if (linkedSource && sameUrl(task.url, linkedSource)) return true;
    return unifiedAlbumKey(task.artistName, task.albumTitle) === unifiedAlbumKey(artist, album.title);
  });
  await Promise.all(matching.map((task) => cancel(task.id)));
  const albums = await fetchAlbums(artist);
  return albums.find((item) => normalizeAlbumTitle(item.title) === normalizeAlbumTitle(album.title))?.songs ?? [];
}

/** Finds the library album title an active download belongs to (by linked source URL first, then normalized album title). */
export function matchDownloadToLibraryAlbum(
  d: Pick<DownloadProgress, "url" | "itemType" | "albumTitle">,
  albums: readonly Pick<Album, "title">[],
  missingSources: Readonly<Record<string, LibraryAlbumSource>>,
  missingReady: boolean,
): string | undefined {
  if (missingReady) {
    const byUrl = Object.entries(missingSources).find(([, source]) =>
      sameUrl(source.source_url, d.url),
    )?.[0];
    if (byUrl !== undefined) return byUrl;
  }
  const norm = normalizeAlbumTitle(d.albumTitle);
  if (!norm) return undefined;
  return albums.find((a) => normalizeAlbumTitle(a.title) === norm)?.title;
}

export interface IntegratedDownloadingTrack {
  taskId: string;
  title: string;
  trackNumber: number;
  discNumber: number;
  percent: number;
  status: "pending" | "idle" | "downloading" | "completed" | "failed" | "cancelled";
}

export function unifiedAlbumKey(artist: string, albumTitle: string): string {
  return `${artist.trim().toLowerCase()}::${normalizeAlbumTitle(albumTitle)}`;
}

export function downloadingAlbumMetaLine(
  d: Pick<DownloadProgress, "releaseYear">,
): string | null {
  return typeof d.releaseYear === "number" && d.releaseYear > 0
    ? String(d.releaseYear)
    : null;
}

/**
 * Extracts downloading tracks belonging to `album` that are not already represented by a saved
 * library song or an already-displayed missing-track row.
 */
export function extraDownloadingTracksForAlbum(
  albumSongs: readonly Pick<Song, "title">[],
  missingTrackTitles: readonly string[],
  downloadsForAlbum: readonly DownloadProgress[],
): IntegratedDownloadingTrack[] {
  const existingTitles = new Set<string>();
  for (const s of albumSongs) existingTitles.add(normalizeAlbumTitle(s.title));
  for (const t of missingTrackTitles) existingTitles.add(normalizeAlbumTitle(t));

  const out: IntegratedDownloadingTrack[] = [];
  const seenDownloading = new Set<string>();

  for (const d of downloadsForAlbum) {
    if (d.tracks && d.tracks.length > 0) {
      d.tracks.forEach((tr, idx) => {
        if (tr.status === "cancelled") return;
        const key = normalizeAlbumTitle(tr.title);
        if (!key || existingTitles.has(key) || seenDownloading.has(key)) return;
        seenDownloading.add(key);
        out.push({
          taskId: d.id,
          title: tr.title,
          trackNumber: tr.trackNumber ?? idx + 1,
          discNumber: tr.discNumber ?? 1,
          percent: tr.percent ?? 0,
          status: tr.status,
        });
      });
    } else if (d.placement?.title) {
      const key = normalizeAlbumTitle(d.placement.title);
      if (key && !existingTitles.has(key) && !seenDownloading.has(key)) {
        seenDownloading.add(key);
        out.push({
          taskId: d.id,
          title: d.placement.title,
          trackNumber: d.placement.track_number ?? albumSongs.length + 1,
          discNumber: 1,
          percent: d.overallPercent ?? 0,
          status: d.status === "completed" ? "completed" : "downloading",
        });
      }
    }
  }

  out.sort((a, b) => a.discNumber - b.discNumber || a.trackNumber - b.trackNumber);
  return out;
}
