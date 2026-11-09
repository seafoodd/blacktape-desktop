import type { AlbumTrackInfo, AlbumPreview } from "@/shared/lib/albumPreview.ts";
import type { LibraryAlbumSource, MissingTracks } from "@/shared/lib/libraryIndex.ts";
import {
  albumTrackToRemoteTrack,
  remoteTrackToStreamItem,
  type RemoteTrack,
} from "@/shared/lib/remoteTrack.ts";
import type { DownloadProgress, SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { Song } from "@/shared/types.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import { sameUrl } from "@/shared/lib/urlKey.ts";
import type { MenuItemOption } from "@/shared/ui/context-menu";
import { playableMissingTracks } from "@/features/library/model/missingTracksStore.ts";

function albumOf(
  missing: MissingTracks,
): Pick<AlbumPreview, "platform" | "url" | "title" | "artist" | "cover_url"> {
  return {
    platform: missing.platform,
    url: missing.source_url,
    title: missing.album_title,
    artist: missing.album_artist,
    cover_url: missing.cover_url,
  };
}

/**
 * The missing songs as remote tracks of their album. `album_url` is the source url, so "Download" in the menu
 * uses the album pipeline and the song lands in the album folder.
 */
export function missingRemoteTracks(missing: MissingTracks): RemoteTrack[] {
  const album = albumOf(missing);
  return playableMissingTracks(missing).map((track) => albumTrackToRemoteTrack(album, track));
}

/** The remote track of one missing song. */
export function missingRemoteTrack(missing: MissingTracks, track: AlbumTrackInfo): RemoteTrack {
  return albumTrackToRemoteTrack(albumOf(missing), track);
}

/** The missing songs as search items (what the stream store plays as a queue). */
export function missingStreamItems(missing: MissingTracks): SearchSuggestion[] {
  return missingRemoteTracks(missing).map(remoteTrackToStreamItem);
}

/** Track number of a missing song, two digits. Unknown numbers fall back to the position in the album. */
export function missingTrackNumber(track: Pick<AlbumTrackInfo, "track_number" | "index">): string {
  return String(track.track_number ?? track.index + 1).padStart(2, "0");
}

export interface MissingMenuActions {
  toggle: () => void;
  download: () => void;
  /** Opens the search for the remote album of a library album that has no source. */
  find?: () => void;
  /** Removes the link of a library album to its remote album. */
  unlink?: () => void;
}

const PLATFORM_NAMES: Record<string, string> = {
  youtube: "YouTube",
  bandcamp: "Bandcamp",
  soundcloud: "SoundCloud",
};

/** "Bandcamp · 2019": platform and year of an album candidate. */
export function candidateMeta(candidate: { platform: unknown; year: number | null }): string {
  const raw = String(candidate.platform);
  const platform = PLATFORM_NAMES[raw.toLowerCase()] ?? raw;
  return candidate.year ? `${platform} \u00b7 ${candidate.year}` : platform;
}

/**
 * Menu entries of a library album that has a source: "Show missing songs (N)" / "Hide missing songs" and
 * "Download missing (N)". Empty when the album has no source. The show entry is left out when there is nothing
 * missing and nothing shown, and the download entry when nothing is missing.
 */
export function missingAlbumMenuItems(
  source: LibraryAlbumSource | undefined,
  shown: boolean,
  actions: MissingMenuActions,
): MenuItemOption[] {
  if (!source) {
    return actions.find
      ? [{ separator: true, label: "" }, { label: "Find missing songs...", action: actions.find }]
      : [];
  }
  const items: MenuItemOption[] = [{ separator: true, label: "" }];
  if (source.missing > 0 || shown) {
    items.push({
      label: shown ? "Hide missing songs" : `Show missing songs (${source.missing})`,
      action: actions.toggle,
    });
  }
  if (source.missing > 0) {
    items.push({
      label: `Download missing (${source.missing})`,
      action: actions.download,
    });
  }
  // the link can be changed when it is wrong (another platform, another edition)
  if (actions.find || actions.unlink) {
    if (actions.find) items.push({ label: "Change linked album...", action: actions.find });
    if (actions.unlink) items.push({ label: "Unlink album", action: actions.unlink });
  }
  return items.length > 1 ? items : [];
}

/** A row of a library album: a downloaded song or a missing one. */
export type AlbumRow =
  | { kind: "song"; song: Song }
  | { kind: "missing"; track: AlbumTrackInfo };

/**
 * The songs and the missing songs of an album in track order (1..N). A song without a track number stays right
 * after the song before it; the order of equal numbers is songs first.
 */
export function mergeAlbumRows(songs: readonly Song[], missing: readonly AlbumTrackInfo[]): AlbumRow[] {
  const keyed: { row: AlbumRow; key: number; order: number }[] = [];
  let last = 0;
  songs.forEach((song, i) => {
    const key = song.track_number ?? last + 0.5;
    last = key;
    keyed.push({ row: { kind: "song", song }, key, order: i });
  });
  missing.forEach((track, i) => {
    keyed.push({
      row: { kind: "missing", track },
      key: track.track_number ?? track.index + 1,
      order: songs.length + i,
    });
  });
  keyed.sort((a, b) => a.key - b.key || a.order - b.order);
  return keyed.map((k) => k.row);
}

function normalizeTitle(title: string): string {
  return title.trim().toLowerCase();
}

/**
 * Progress (0..100) of a missing song inside the running download of its album: 0 while it waits or when the
 * download does not list it. Tracks are matched by title, then by track number.
 */
export function missingTrackPercent(
  download: Pick<DownloadProgress, "tracks"> | undefined,
  track: Pick<AlbumTrackInfo, "title" | "track_number" | "index">,
): number {
  if (!download) return 0;
  const title = normalizeTitle(track.title);
  const number = track.track_number ?? track.index + 1;
  const found =
    download.tracks.find((t) => normalizeTitle(t.title) === title) ??
    download.tracks.find((t) => t.trackNumber === number);
  if (!found) return 0;
  return found.status === "completed" ? 100 : Math.max(0, Math.min(100, Math.round(found.percent)));
}

/**
 * True when the running download will fetch this track: it lists the track (by title, then by number), or it does
 * not list its tracks yet (the whole album is on its way).
 */
export function missingTrackDownloading(
  download: Pick<DownloadProgress, "tracks" | "status"> | undefined,
  track: Pick<AlbumTrackInfo, "title" | "track_number" | "index">,
): boolean {
  if (!download) return false;
  if (download.status !== "downloading" && download.status !== "idle" && download.status !== "processing") {
    return false;
  }
  if (download.tracks.length === 0) return true;
  const title = normalizeTitle(track.title);
  const number = track.track_number ?? track.index + 1;
  return download.tracks.some((t) => normalizeTitle(t.title) === title || t.trackNumber === number);
}

/** Marks a track that is only a placeholder of a song that was just removed from the library (no url, no actions). */
export const GHOST_URL = "";

/** How long a placeholder waits for the missing-songs list to catch up with a delete. */
export const GHOST_TTL_MS = 3000;

/** Lowercase words only: text in (), [] removed ("(+ Guest)"), punctuation turned into spaces. */
export function looseTitle(title: string): string {
  return title
    .replace(/\([^)]*\)|\[[^\]]*\]/g, " ")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

const hasBrackets = (title: string): boolean => /\([^)]*\)|\[[^\]]*\]/.test(title);

/**
 * Same song by title. Brackets are ignored on the song side ("Title(+ Guest)" is "Title"), but not on the remote
 * side: "Title (interlude)" is another track than the song "Title".
 */
function sameTitle(songTitle: string, trackTitle: string): boolean {
  if (songTitle.trim().toLowerCase() === trackTitle.trim().toLowerCase()) return true;
  if (hasBrackets(trackTitle) && !hasBrackets(songTitle)) return false;
  const [x, y] = [looseTitle(songTitle), looseTitle(trackTitle)];
  return x !== "" && x === y;
}

/** True when the library song and the remote track are the same song (same page, else same title). */
export function songMatchesTrack(song: Pick<Song, "title" | "source_url">, track: Pick<AlbumTrackInfo, "title" | "url">): boolean {
  return (track.url !== "" && sameUrl(song.source_url, track.url)) || sameTitle(song.title, decodeHtmlEntities(track.title));
}

/**
 * The missing tracks that are not in the library songs yet. Right after a download the library list can be
 * ahead of the missing list; without this the song would be shown twice for a moment.
 */
export function withoutDownloaded(missing: readonly AlbumTrackInfo[], songs: readonly Song[]): AlbumTrackInfo[] {
  return missing.filter((t) => !songs.some((s) => songMatchesTrack(s, t)));
}

/** A removed song that stays in place as a dimmed placeholder until the missing list lists it. */
export interface Ghost {
  song: Song;
  at: number;
}

/**
 * Placeholders of an album after a library change. A song that was in `prevSongs` and is gone from `songs` stays as
 * a placeholder (so the rows do not jump) until the missing list contains it, it comes back, or the placeholder
 * is older than `GHOST_TTL_MS`.
 */
export function nextGhosts(
  prevSongs: readonly Song[] | undefined,
  songs: readonly Song[],
  missing: readonly AlbumTrackInfo[],
  ghosts: ReadonlyMap<number, Ghost>,
  now: number,
): Map<number, Ghost> {
  const ids = new Set(songs.map((s) => s.id));
  const listed = (s: Song) => missing.some((t) => songMatchesTrack(s, t));
  const next = new Map<number, Ghost>();
  for (const [id, ghost] of ghosts) {
    if (!ids.has(id) && !listed(ghost.song) && now - ghost.at < GHOST_TTL_MS) next.set(id, ghost);
  }
  for (const song of prevSongs ?? []) {
    if (!ids.has(song.id) && !next.has(song.id) && !listed(song)) next.set(song.id, { song, at: now });
  }
  return next;
}

/** The placeholder of a removed song as a track (empty url: nothing can be played or downloaded). */
export function ghostTrack(song: Song, index: number): AlbumTrackInfo {
  return {
    index,
    title: song.title,
    artists: [],
    track_number: song.track_number ?? null,
    url: GHOST_URL,
    available: true,
    note: null,
    duration_ms: song.duration_ms,
  };
}
