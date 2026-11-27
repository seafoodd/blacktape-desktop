/**
 * Album preview: the track list of an album/set before downloading. Types mirror `AlbumPreview` in
 * `src-tauri/src/download/album_preview.rs` (serde, snake_case); the helpers are pure.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  albumStateFromTracks,
  type DownloadState,
  type RemoteRef,
} from "@/shared/lib/library/libraryIndex.ts";
import { formatTotalDuration, formatTrackDuration } from "@/shared/lib/library/trackDuration.ts";
import { sameUrl } from "@/shared/lib/remote/urlKey.ts";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { Platform } from "@/shared/types.ts";
import { platformForUrl } from "@/shared/lib/remote/platforms.ts";
import type { AudioCandidate, ReleaseEditionOption } from "@/shared/types/canonical.ts";

export interface AlbumTrackInfo {
  /** Position in the album (0-based, also for unavailable rows). */
  index: number;
  title: string;
  artists: string[];
  track_number: number | null;
  disc_number?: number | null;
  /** Page url of the track (the download selects tracks by it); empty when unavailable. */
  url: string;
  available: boolean;
  /** Why the track is unavailable. */
  note: string | null;
  /** Length in ms when the platform provides it. */
  duration_ms?: number | null;
  candidates?: AudioCandidate[];
  selected_candidate_idx?: number;
}

export interface AlbumPreview {
  platform: Platform;
  url: string;
  title: string;
  artist: string;
  cover_url: string | null;
  year: number | null;
  genres: string[];
  tracks: AlbumTrackInfo[];
  /** Sum of the known track durations in ms; null when no track has one. */
  total_duration_ms?: number | null;
  metadata_source?: string;
  is_free_download?: boolean;
  release_group_id?: string | null;
  selected_release_id?: string | null;
  candidates_resolved?: boolean;
}

export function formatEditionSubtext(edition: Pick<ReleaseEditionOption, "year" | "track_count" | "format">): string {
  const parts: string[] = [];
  if (edition.year) parts.push(String(edition.year));
  if (edition.track_count > 0) {
    parts.push(`${edition.track_count} ${edition.track_count === 1 ? "track" : "tracks"}`);
  }
  if (edition.format) parts.push(edition.format);
  return parts.join(" · ");
}

function normalizeMatchText(raw: string | null | undefined): string {
  return (raw ?? "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

/** Whether an album preview row represents the currently playing song (checks local ID, active track URL, or same-album track title across edition switches). Pure. */
export function isTrackRowActive(
  track: Pick<AlbumTrackInfo, "available" | "url" | "title" | "candidates">,
  localSongId: number | undefined,
  currentSong: { id?: number; title?: string | null; album?: string | null; source_url?: string | null } | null | undefined,
  albumTitle?: string | null,
): boolean {
  if (!track.available || !currentSong) return false;
  if (localSongId !== undefined && currentSong.id === localSongId) return true;
  if (sameUrl(currentSong.source_url, track.url)) return true;

  const normTrackTitle = normalizeMatchText(track.title);
  const normCurrentTitle = normalizeMatchText(currentSong.title);
  if (!normTrackTitle || normTrackTitle !== normCurrentTitle) return false;

  const candidateMatched =
    !!currentSong.source_url &&
    (track.candidates?.some(
      (c) => c.source.kind === "http_stream" && sameUrl(currentSong.source_url, c.source.url),
    ) ??
      false);
  if (candidateMatched) return true;

  const normAlbum = normalizeMatchText(albumTitle);
  const normCurrentAlbum = normalizeMatchText(currentSong.album);
  return !!normAlbum && normAlbum === normCurrentAlbum;
}

export interface AlbumMetadataSource {
  label: string;
  kind: "musicbrainz" | "youtube" | "bandcamp" | "soundcloud";
}

export function albumMetadataSourceBadge(
  preview: Pick<AlbumPreview, "platform" | "metadata_source" | "url">,
): AlbumMetadataSource {
  const src = (preview.metadata_source ?? "").toLowerCase();
  if (src === "musicbrainz" || preview.url.startsWith("canonical:rg:")) {
    return { label: "MusicBrainz", kind: "musicbrainz" };
  }
  if (src === "bandcamp" || src.startsWith("bandcamp") || String(preview.platform).toLowerCase() === "bandcamp") {
    return { label: "Bandcamp", kind: "bandcamp" };
  }
  if (src === "soundcloud" || src.startsWith("soundcloud") || String(preview.platform).toLowerCase() === "soundcloud") {
    return { label: "SoundCloud", kind: "soundcloud" };
  }
  return { label: "YouTube", kind: "youtube" };
}

export interface EstimatedTrackQuality {
  label: string;
  tier: "lossless" | "high" | "standard";
}

export function estimatedPlatformQuality(
  preview: Pick<AlbumPreview, "platform" | "metadata_source" | "is_free_download">,
  trackUrl: string,
): EstimatedTrackQuality {
  const fallback = (preview.metadata_source ?? "").toLowerCase() === "musicbrainz" ? "youtube" : preview.platform;
  const plat = platformForUrl(trackUrl, fallback);

  if (plat === "bandcamp") {
    return preview.is_free_download
      ? { label: "~ BC · Lossless", tier: "lossless" }
      : { label: "~ BC · MP3 128k", tier: "standard" };
  }
  if (plat === "soundcloud") {
    return { label: "~ SC · MP3 128k", tier: "standard" };
  }
  return { label: "~ YT · OPUS (High)", tier: "high" };
}

export function applyMetadataPatch(
  preview: AlbumPreview,
  patch: {
    title?: string;
    artist?: string;
    year?: number | null;
    tracks?: Array<{ index: number; title: string }>;
  },
): AlbumPreview {
  const updatedTracks = preview.tracks.map((t) => {
    const override = patch.tracks?.find((p) => p.index === t.index);
    return override ? { ...t, title: override.title } : t;
  });

  return {
    ...preview,
    title: patch.title !== undefined ? patch.title : preview.title,
    artist: patch.artist !== undefined ? patch.artist : preview.artist,
    year: patch.year !== undefined ? patch.year : preview.year,
    tracks: updatedTracks,
  };
}

export interface MetadataSnapshot {
  title: string;
  artist: string;
  year: number | null;
  trackTitles: Record<number, string>;
}

export function snapshotMetadata(preview: AlbumPreview): MetadataSnapshot {
  const trackTitles: Record<number, string> = {};
  for (const t of preview.tracks) {
    trackTitles[t.index] = t.title;
  }
  return {
    title: preview.title,
    artist: preview.artist,
    year: preview.year ?? null,
    trackTitles,
  };
}

export function hasMetadataChanges(
  preview: AlbumPreview | null,
  original: MetadataSnapshot | null,
): boolean {
  if (!preview || !original) return false;
  if (
    preview.title !== original.title ||
    preview.artist !== original.artist ||
    (preview.year ?? null) !== original.year
  ) {
    return true;
  }
  return preview.tracks.some((t) => t.title !== (original.trackTitles[t.index] ?? t.title));
}

export interface AlbumPreviewFallbackMeta {
  artist?: string;
  title?: string;
  coverUrl?: string;
}

/** Fetches the track list of the album at `url`. Rejects with a message string. */
export const getAlbumPreview = (
  platform: Platform,
  url: string,
  skipMbUpgrade?: boolean,
  fallbackMeta?: AlbumPreviewFallbackMeta,
): Promise<AlbumPreview> =>
  invoke<AlbumPreview>("get_album_preview", {
    platform,
    url,
    ...(skipMbUpgrade ? { skipMbUpgrade } : {}),
    ...(fallbackMeta?.artist ? { fallbackArtist: fallbackMeta.artist } : {}),
    ...(fallbackMeta?.title ? { fallbackTitle: fallbackMeta.title } : {}),
    ...(fallbackMeta?.coverUrl ? { fallbackCoverUrl: fallbackMeta.coverUrl } : {}),
  });

/** The tracks that can be selected and downloaded, in album order. */
export function selectableTracks(preview: Pick<AlbumPreview, "tracks">): AlbumTrackInfo[] {
  return preview.tracks.filter((t) => t.available && t.url !== "");
}

/** Indexes of the tracks that can be selected and downloaded, in album order. */
export function selectableIndexes(preview: Pick<AlbumPreview, "tracks">): number[] {
  return selectableTracks(preview).map((t) => t.index);
}

/** Page urls of the selected tracks, in album order (unavailable or unknown indexes are dropped). */
export function selectedUrls(
  preview: Pick<AlbumPreview, "tracks">,
  selected: ReadonlySet<number>,
): string[] {
  return preview.tracks
    .filter((t) => selected.has(t.index) && t.available && t.url !== "")
    .map((t) => t.url);
}

/** "Download album", "Download track" or "Download 3 of 10 tracks". */
export function downloadLabel(selectedCount: number, availableCount: number): string {
  if (selectedCount === 0 || selectedCount >= availableCount) {
    return availableCount === 1 ? "Download track" : "Download album";
  }
  return selectedCount === 1
    ? "Download 1 track"
    : `Download ${selectedCount} of ${availableCount} tracks`;
}

/** What the main button of the album preview does. */
export type AlbumAction =
  | { kind: "download"; label: string; urls?: undefined }
  | { kind: "download-missing"; label: string; urls: string[] }
  | { kind: "go-to-album"; label: "Go to album"; urls?: undefined };

/**
 * The main button: with a selection it downloads the selection (`downloadLabel`); without one it depends on what
 * the library already has of the album: everything -> "Go to album", some tracks -> download only the missing
 * ones, nothing -> the whole album. `states` is the library state of every track url. Pure.
 */
export function albumAction(
  preview: Pick<AlbumPreview, "tracks">,
  states: Readonly<Record<string, DownloadState>>,
  selectedCount: number,
): AlbumAction {
  const tracks = selectableTracks(preview);
  if (selectedCount > 0 || tracks.length === 0) {
    return { kind: "download", label: downloadLabel(selectedCount, tracks.length) };
  }
  const missing = tracks.filter((t) => (states[t.url] ?? "none") !== "downloaded").map((t) => t.url);
  if (missing.length === 0) return { kind: "go-to-album", label: "Go to album" };
  if (missing.length === tracks.length) return { kind: "download", label: downloadLabel(0, tracks.length) };
  return {
    kind: "download-missing",
    label: missing.length === 1 ? "Download missing track" : "Download missing tracks",
    urls: missing,
  };
}

/** "2020 · Ambient · 1 hr 5 min · 10 tracks": the album info line followed by the track count. */
export function albumMetaLine(
  preview: Pick<AlbumPreview, "year" | "genres" | "tracks"> &
    Partial<Pick<AlbumPreview, "total_duration_ms">>,
): string {
  return [albumInfoLine(preview), trackCountLabel(preview)].filter((p) => p !== "").join(" · ");
}

/** "10 tracks", "1 track", "10 tracks · 2 unavailable". */
export function trackCountLabel(preview: Pick<AlbumPreview, "tracks">): string {
  const total = preview.tracks.length;
  const unavailable = preview.tracks.filter((t) => !t.available).length;
  const base = `${total} ${total === 1 ? "track" : "tracks"}`;
  return unavailable > 0 ? `${base} · ${unavailable} unavailable` : base;
}

/** "2020 · Ambient, Drone · 1 hr 5 min" (parts that are missing are skipped). */
export function albumInfoLine(
  preview: Pick<AlbumPreview, "year" | "genres"> & Partial<Pick<AlbumPreview, "total_duration_ms">>,
): string {
  return [
    preview.year ? String(preview.year) : "",
    preview.genres.join(", "),
    formatTotalDuration(preview.total_duration_ms),
  ]
    .filter((p) => p !== "")
    .join(" · ");
}

/** Resolves the true streaming platform for an album preview track URL (avoiding YouTube fallback on Bandcamp/SoundCloud URLs). Pure. */
export function platformForTrackUrl(
  url: string,
  fallback: Platform,
  track?: Pick<AlbumTrackInfo, "candidates" | "selected_candidate_idx">,
): Platform {
  const detected = platformForUrl(url, "local");
  if (detected !== "local") return detected;
  if (track) {
    const selected = selectedTrackCandidate(track);
    if (selected?.source.kind === "http_stream") {
      const p = selected.source.platform.toLowerCase();
      if (p === "bandcamp" || p === "soundcloud" || p === "youtube") return p;
    }
  }
  return fallback;
}

/** The track as a search item, so the stream button can play it (and queue the tracks after it). */
export function trackAsStreamItem(
  preview: Pick<AlbumPreview, "platform" | "title" | "artist" | "cover_url"> & Partial<Pick<AlbumPreview, "url">>,
  track: AlbumTrackInfo,
): SearchSuggestion {
  return {
    item_type: ItemType.Track,
    name: track.title,
    band_name: track.artists[0] || preview.artist,
    album_name: preview.title,
    album_url: preview.url,
    item_url_path: track.url,
    img: preview.cover_url ?? "",
    duration: typeof track.duration_ms === "number" ? Math.round(track.duration_ms / 1000) : undefined,
    platform: platformForTrackUrl(track.url, preview.platform, track),
  };
}

/** Library-index refs of the selectable tracks (what the status hooks are asked about). */
export function trackRefs(preview: Pick<AlbumPreview, "platform" | "artist" | "title" | "tracks">): RemoteRef[] {
  return selectableTracks(preview).map((t) => ({
    platform: preview.platform,
    url: t.url,
    artist: t.artists[0] || preview.artist,
    title: t.title,
    album: preview.title || undefined,
  }));
}

/**
 * Library state of the album, from the states of its selectable tracks (D7: the album preview derives it from
 * the tracks, which also covers albums whose tracks were downloaded one by one). Missing states count as "none".
 */
export function albumLibraryState(
  preview: Pick<AlbumPreview, "tracks">,
  states: Readonly<Record<string, DownloadState>>,
): DownloadState {
  return albumStateFromTracks(selectableTracks(preview).map((t) => states[t.url] ?? "none"));
}

/** Progress of a download task that serves some rows of this page. */
export type TrackTaskState = { phase: "running"; percent: number; taskId?: string } | { phase: "done" };

/** Download task statuses that still do work (the library keeps other tasks only as history). */
const RUNNING_STATUSES: ReadonlySet<string> = new Set(["idle", "downloading", "processing"]);

const clampPercent = (value: number): number =>
  Number.isFinite(value) ? Math.min(100, Math.max(0, Math.round(value))) : 0;

export interface ActiveDownloadInfo {
  status: string;
  overallPercent: number;
  tracks?: readonly { title: string; percent: number; status: string; trackNumber?: number }[];
  placement?: { title: string; track_number: number | null };
  trackUrls?: readonly string[];
  url?: string;
}

/**
 * Rows served by the tasks of this page: track url -> state. `taskTracks` maps a task id to the track urls it
 * downloads; `active` is the library's `activeDownloads`. When `previewTracks` is available and the task reports
 * individual track progress, each row shows its own percentage (like in `ArtistAlbums`).
 */
export function trackTaskStates(
  taskTracks: Readonly<Record<string, readonly string[]>>,
  active: Readonly<Record<string, ActiveDownloadInfo>>,
  previewTracks?: readonly AlbumTrackInfo[],
  previewUrl?: string,
): Record<string, TrackTaskState> {
  const out: Record<string, TrackTaskState> = {};
  const mergedTaskTracks: Record<string, readonly string[]> = { ...taskTracks };

  // If this album has an active download running in the library (e.g. started from search results
  // or a previous visit), ensure its tracks are mapped.
  if (previewUrl && previewTracks) {
    for (const [taskId, task] of Object.entries(active)) {
      if (task.url === previewUrl && !mergedTaskTracks[taskId]) {
        mergedTaskTracks[taskId] =
          task.trackUrls && task.trackUrls.length > 0
            ? task.trackUrls
            : previewTracks.map((t) => t.url);
      }
    }
  }

  for (const [taskId, urls] of Object.entries(mergedTaskTracks)) {
    const task = active[taskId];
    if (!task) continue;

    if (task.status === "completed") {
      for (const url of urls) out[url] = { phase: "done" };
      continue;
    }

    if (!RUNNING_STATUSES.has(task.status)) continue;

    for (const url of urls) {
      if (task.tracks && task.tracks.length > 0 && previewTracks) {
        const pTrack = previewTracks.find((t) => t.url === url);
        if (pTrack) {
          const title = pTrack.title.trim().toLowerCase();
          const num = pTrack.track_number ?? pTrack.index + 1;
          const found =
            task.tracks.find((t) => t.title.trim().toLowerCase() === title) ??
            task.tracks.find((t) => t.trackNumber === num);
          if (found) {
            if (found.status === "completed" || found.percent >= 100) {
              out[url] = { phase: "running", taskId, percent: 100 };
            } else {
              out[url] = { phase: "running", taskId, percent: clampPercent(found.percent) };
            }
            continue;
          } else {
            // Track belongs to the download but hasn't started yet
            out[url] = { phase: "running", taskId, percent: 0 };
            continue;
          }
        }
      }

      out[url] = { phase: "running", taskId, percent: clampPercent(task.overallPercent) };
    }
  }

  return out;
}

export type TrackRowStatus =
  | { kind: "unavailable" }
  | { kind: "downloaded" }
  | { kind: "running"; percent: number }
  | { kind: "download" };

/**
 * What the action slot of a track row shows. The library state wins over a running task: a track that is in the
 * library is downloaded even while another download of the album still runs.
 */
export function trackRowStatus(
  track: Pick<AlbumTrackInfo, "available" | "url">,
  libraryState: DownloadState | undefined,
  task: TrackTaskState | undefined,
): TrackRowStatus {
  if (!track.available || track.url === "") return { kind: "unavailable" };
  if (libraryState === "downloaded" || task?.phase === "done") return { kind: "downloaded" };
  if (task?.phase === "running") return { kind: "running", percent: task.percent };
  return { kind: "download" };
}

/** Selected candidate for a track row, falling back to candidate 0. Pure. */
function selectedTrackCandidate(
  track: Pick<AlbumTrackInfo, "candidates" | "selected_candidate_idx">,
): AudioCandidate | undefined {
  const cands = track.candidates ?? [];
  if (cands.length === 0) return undefined;
  const idx =
    typeof track.selected_candidate_idx === "number" &&
    track.selected_candidate_idx >= 0 &&
    track.selected_candidate_idx < cands.length
      ? track.selected_candidate_idx
      : 0;
  return cands[idx] ?? cands[0];
}

/** Human-readable mismatch summary when a candidate is not a strict match; null when strict. Pure. */
export function candidateMatchWarning(
  track: Pick<AlbumTrackInfo, "title" | "duration_ms">,
  cand: Pick<AudioCandidate, "is_strict_match" | "matched_title" | "matched_artist" | "duration_ms">,
): string | null {
  if (cand.is_strict_match) return null;
  const details: string[] = [];
  if (cand.matched_title) {
    const target = normalizeMatchText(track.title);
    const matched = normalizeMatchText(cand.matched_title);
    if (matched && matched !== target) {
      details.push(`source "${cand.matched_title}"`);
    }
  }
  if (track.duration_ms && track.duration_ms > 0 && cand.duration_ms > 0) {
    const deltaSecs = Math.round(Math.abs(cand.duration_ms - track.duration_ms) / 1000);
    if (deltaSecs >= 4) {
      details.push(`${formatTrackDuration(cand.duration_ms)} vs ${formatTrackDuration(track.duration_ms)}`);
    }
  }
  return details.length > 0
    ? `Unverified match (${details.join(" · ")})`
    : "Approximate title or duration match";
}

/** Numeric rank of a local QualityTier or AudioQualityTier (`0..3`). Pure. */
export function audioTierRank(tier: string | null | undefined): number {
  switch ((tier ?? "").toLowerCase()) {
    case "lossless":
      return 3;
    case "high":
    case "high_lossy":
      return 2;
    case "standard":
    case "standard_lossy":
      return 1;
    default:
      return 0;
  }
}

/** Downloaded tracks on `AlbumPreview` that have a strictly higher-quality resolved candidate. Pure. */
export function upgradableAlbumTracks<
  TLocal extends { id: number; quality_tier: string; path?: string },
>(
  preview: Pick<AlbumPreview, "tracks">,
  localTracks: Readonly<Record<string, TLocal>>,
): Array<{ track: AlbumTrackInfo; local: TLocal; candidate: AudioCandidate }> {
  const out: Array<{ track: AlbumTrackInfo; local: TLocal; candidate: AudioCandidate }> = [];
  for (const track of selectableTracks(preview)) {
    const local = localTracks[track.url];
    if (!local) continue;
    const candidate = selectedTrackCandidate(track);
    if (!candidate) continue;
    if (audioTierRank(candidate.quality_tier) > audioTierRank(local.quality_tier)) {
      out.push({ track, local, candidate });
    }
  }
  return out;
}

/** Downloadable tracks whose selected audio candidate is non-strict (`!is_strict_match`). Pure. */
export function unverifiedTracksForDownload(
  preview: Pick<AlbumPreview, "tracks">,
  targetUrls?: readonly string[],
): Array<{ track: AlbumTrackInfo; candidate: AudioCandidate; warning: string }> {
  const urlSet = targetUrls ? new Set(targetUrls) : null;
  const out: Array<{ track: AlbumTrackInfo; candidate: AudioCandidate; warning: string }> = [];
  for (const track of selectableTracks(preview)) {
    if (urlSet && !urlSet.has(track.url)) continue;
    const cand = selectedTrackCandidate(track);
    if (!cand || cand.is_strict_match) continue;
    out.push({
      track,
      candidate: cand,
      warning: candidateMatchWarning(track, cand) ?? "Approximate match",
    });
  }
  return out;
}

export interface PartitionedEditions {
  visibleMb: ReleaseEditionOption[];
  hiddenMbCount: number;
  platformEditions: ReleaseEditionOption[];
}

/** Splits MusicBrainz releases into primary vs alternate (vinyl/mono/retitled/duplicate 1×CD) and platform options. */
export function partitionEditions(
  editions: readonly ReleaseEditionOption[],
  selectedReleaseId: string | null | undefined,
  showAll: boolean,
): PartitionedEditions {
  const mb = editions.filter((e) => !e.source_kind || e.source_kind === "musicbrainz");
  const platformEditions = editions.filter((e) => e.source_kind && e.source_kind !== "musicbrainz");
  const primary = mb.filter((e) => !e.is_alternate || (selectedReleaseId != null && e.id === selectedReleaseId));
  const useAll = showAll || primary.length === 0;
  return {
    visibleMb: useAll ? mb : primary,
    hiddenMbCount: primary.length === 0 ? 0 : Math.max(0, mb.length - primary.length),
    platformEditions,
  };
}
