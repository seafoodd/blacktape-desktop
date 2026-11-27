import { Channel, invoke } from "@tauri-apps/api/core";
import { Album, ArtistSummary, RepeatMode, Song } from "@/shared/types";
import {
  AudioCandidate,
  AudioQualityTier,
  CanonicalRelease,
  CanonicalReleaseSummary,
  CanonicalTrack,
  ReleaseEditionOption,
} from "@/shared/types/canonical";

export interface TorrentTrack {
  file_index: number;
  file_path: string;
  title: string;
  codec: string;
  size_bytes: number;
  disc_number: number | null;
  track_number: number | null;
  is_sample?: boolean;
}

export interface TorrentReleaseHit {
  id: string;
  title: string;
  input: string;
  provider: string;
  seeders: number | null;
  size_bytes: number | null;
  rank: number;
}

export interface TorrentProviderReport {
  provider: string;
  hits: TorrentReleaseHit[];
  error: string | null;
}

export interface TorrentSearchReport {
  providers: TorrentProviderReport[];
  hits: TorrentReleaseHit[];
}

export interface TorrentIndexer {
  id: "archive" | "knaben" | "api_bay" | "solid_torrents" | "rutor" | "ru_tracker";
  name: string;
  enabled: boolean;
}

export interface TorrentIndexerTest {
  hits: number;
  error: string | null;
}

export const listTorrentIndexers = (): Promise<TorrentIndexer[]> =>
  invoke("plugin:torrent|list_torrent_indexers");

export const setTorrentIndexerEnabled = (id: TorrentIndexer["id"], enabled: boolean): Promise<TorrentIndexer[]> =>
  invoke("plugin:torrent|set_torrent_indexer_enabled", { id, enabled });

export const testTorrentIndexer = (id: TorrentIndexer["id"]): Promise<TorrentIndexerTest> =>
  invoke("plugin:torrent|test_torrent_indexer", { id });

export const openRuTrackerLogin = (): Promise<void> => invoke("plugin:torrent|open_rutracker_login");

export interface InspectedTorrentRelease {
  info_hash: string;
  name: string;
  layout: string;
  tracks: TorrentTrack[];
}

export function searchTorrentReleases(
  artist: string,
  title: string,
  onProvider: (report: TorrentProviderReport) => void,
): Promise<TorrentSearchReport> {
  const progress = new Channel<TorrentProviderReport>(onProvider);
  return invoke("plugin:torrent|search_torrent_releases", { artist, title, progress });
}

export interface ExpectedTorrentTrackInput {
  index: number;
  title: string;
  artist?: string | null;
  disc_number?: number | null;
  track_number?: number | null;
  duration_ms?: number | null;
}

export interface TorrentTierEstimate {
  tier: AudioQualityTier;
  codec: string;
  bitrate_kbps: number | null;
  label: string;
  release_title: string;
  provider: string;
  seeders: number | null;
  size_bytes: number | null;
}

interface MatchedTorrentTrack {
  expected_index: number;
  candidate: AudioCandidate;
}

export interface TorrentTierResolution {
  tier: AudioQualityTier;
  info_hash: string;
  magnet_uri: string;
  release_title: string;
  provider: string;
  seeders: number | null;
  codec: string;
  bitrate_kbps: number | null;
  display_label: string;
  matched_count: number;
  total_expected: number;
  total_matched_bytes: number;
  missing_titles: string[];
  ambiguous_titles: string[];
  layout_warning: string | null;
  tracks: MatchedTorrentTrack[];
}

export type TorrentResolveEvent =
  | { kind: "searching" }
  | { kind: "search_progress" | "inspection_progress"; checked: number; total: number }
  | { kind: "estimated"; estimates: TorrentTierEstimate[] }
  | { kind: "verified"; resolutions: TorrentTierResolution[] }
  | { kind: "resolved"; resolutions: TorrentTierResolution[]; unavailable_providers: string[] };

export const inspectTorrentRelease = (input: string, requestId: string): Promise<InspectedTorrentRelease> =>
  invoke("plugin:torrent|inspect_torrent_release", { input, requestId });

export function resolveTorrentAlbum(
  artist: string,
  title: string,
  expected: ExpectedTorrentTrackInput[],
  requestId: string,
  onEvent: (event: TorrentResolveEvent) => void,
): Promise<[TorrentTierResolution[], string[]]> {
  const progress = new Channel<TorrentResolveEvent>(onEvent);
  return invoke("plugin:torrent|resolve_torrent_album", {
    artist,
    title,
    expected,
    requestId,
    progress,
  });
}

export const cancelTorrentInspection = (requestId: string): Promise<void> =>
  invoke("plugin:torrent|cancel_torrent_inspection", { requestId });

export type CancelTrackOutcome =
  | "cancelled"
  | "already_saved"
  | "too_late_saving"
  | "already_cancelled"
  | "not_found";

export const cancelDownloadTrack = (
  taskId: string,
  trackIndex: number,
): Promise<CancelTrackOutcome> =>
  invoke<CancelTrackOutcome>("cancel_download_track", { taskId, trackIndex });

export const scanMusic = (dir: string): Promise<Song[]> =>
  invoke<Song[]>("scan_music", { dir });

export const getArtists = (query?: string): Promise<ArtistSummary[]> => {
  if (query) console.log("Queries not supported");
  return invoke<ArtistSummary[]>("get_artists");
};

export const getArtistAlbums = (artistName: string): Promise<Album[]> =>
  invoke<Album[]>("get_artist_albums", { artistName });

export const startPlayback = (
  queue: number[],
  currentIndex: number,
): Promise<void> => invoke("start_playback", { queue, currentIndex });

export const restorePlaybackState = (
  songs: Song[],
  currentIndex: number,
): Promise<void> => invoke("restore_playback_state", { songs, currentIndex });

export const playSongInQueue = (index: number): Promise<void> =>
  invoke("play_song_in_queue", { index });

/** Removes the songs at these positions of the (play-ordered) queue. */
export const removeFromQueue = (positions: number[]): Promise<void> =>
  invoke("remove_from_queue", { positions });

/** Moves queue positions as a block to just before position `insertBefore`. */
export const moveInQueue = (
  positions: number[],
  insertBefore: number,
): Promise<void> => invoke("move_in_queue", { positions, insertBefore });

export const pause = (): Promise<void> => invoke("pause");

export const resume = (): Promise<void> => invoke("resume");

export const toggle = (): Promise<void> => invoke("toggle");

export const seek = async (fraction: number): Promise<void> => {
  try {
    await invoke("seek", { fraction });
  } catch (e) {
    // the backend says why the position did not move (see the [seek] lines of its log)
    console.warn("[seek] failed:", e);
  }
  await resume();
};

export const next = (): Promise<void> => invoke("next");

export const previous = (): Promise<void> => invoke("previous");

export const AddToQueue = (id: number): Promise<void> =>
  invoke("add_to_queue", { songId: id });

export const AddNextToQueue = (id: number): Promise<void> =>
  invoke("add_next_to_queue", { songId: id });

export const getPosition = (): Promise<number> => {
  return invoke<number>("get_position");
};

export const setVolume = (fraction: number): Promise<void> =>
  invoke("set_volume", { fraction });

export const fetchState = (): void => {
  invoke("fetch_state");
};
export const toggleShuffle = (): void => {
  invoke("toggle_shuffle");
};

export const setRepeatMode = (repeatMode: RepeatMode): void => {
  invoke("set_repeat_mode", { repeatMode: repeatMode });
};

/** Whether this platform can blur what is behind the window (Windows and macOS). */
export const windowBlurSupported = (): Promise<boolean> =>
  invoke<boolean>("window_blur_supported");

/** Turns the native blur behind the calling window on or off; `tint` is the theme background. */
export const setWindowBlur = (
  enabled: boolean,
  tint: [number, number, number],
): Promise<void> => invoke("set_window_blur", { enabled, tint });

/** The helper programs behind downloads and previews. */
export type ToolName = "ytDlp" | "deno" | "ffmpeg";

export interface ToolStatus {
  phase: "checking" | "downloading" | "ready" | "failed" | "unsupported";
  /** A working copy is installed (possibly an older one while an update downloads or after one failed). */
  usable: boolean;
  version: string | null;
  /** "system": installed by the user; "app": the app's own copy. */
  source: "system" | "app" | null;
  /** Bytes on disk of the app's copy. */
  size: number | null;
  downloaded: number;
  total: number | null;
  error: string | null;
}

export type ToolsStatus = Record<ToolName, ToolStatus> & {
  /** Grows with every change: of two snapshots, the higher one is newer. */
  seq: number;
};

/** Setup state of yt-dlp, deno and ffmpeg; later changes arrive as `tools-status` events. */
export const getToolsStatus = (): Promise<ToolsStatus> =>
  invoke<ToolsStatus>("get_tools_status");

/** Looks for newer yt-dlp, deno and ffmpeg releases now and retries setups that failed. */
export const checkTools = (): Promise<void> => invoke("check_tools");

export const getTelemetryEnabled = (): Promise<boolean> =>
  invoke<boolean>("get_telemetry_enabled");

export const setTelemetryEnabled = (enabled: boolean): Promise<void> =>
  invoke("set_telemetry_enabled", { enabled });

export const getOnboardingCompleted = (): Promise<boolean> =>
  invoke<boolean>("get_onboarding_completed");

export type DownloadQualityPreset = "best" | "high" | "standard";

export const getDefaultDownloadQuality = (): Promise<DownloadQualityPreset> =>
  invoke<DownloadQualityPreset>("get_default_download_quality");

export const setDefaultDownloadQuality = (quality: DownloadQualityPreset): Promise<void> =>
  invoke("set_default_download_quality", { quality });

export const setOnboardingCompleted = (completed: boolean): Promise<void> =>
  invoke("set_onboarding_completed", { completed });

export const getFavoriteSongs = (): Promise<Song[]> =>
  invoke<Song[]>("get_favorite_songs");

export const getFavoriteIds = (): Promise<number[]> =>
  invoke<number[]>("get_favorite_ids");

export const toggleFavorite = (songId: number): Promise<boolean> =>
  invoke<boolean>("toggle_favorite", { songId });

export const addFavorites = (songIds: number[]): Promise<void> =>
  invoke("add_favorites", { songIds });

export const removeFavorites = (songIds: number[]): Promise<void> =>
  invoke("remove_favorites", { songIds });

export const searchCanonicalReleases = (
  query: string,
): Promise<CanonicalReleaseSummary[]> =>
  invoke<CanonicalReleaseSummary[]>("search_canonical_releases", { query });

export const getCanonicalRelease = (id: string): Promise<CanonicalRelease> =>
  invoke<CanonicalRelease>("get_canonical_release", { id });

export const getReleaseGroupReleases = (
  rgId?: string | null,
  artist?: string,
  title?: string,
): Promise<ReleaseEditionOption[]> =>
  invoke<ReleaseEditionOption[]>("get_release_group_releases", {
    rgId: rgId || undefined,
    artist: artist || undefined,
    title: title || undefined,
  });

export const resolveCanonicalCandidates = (
  track: CanonicalTrack,
  fallbackArtist?: string,
): Promise<CanonicalTrack> =>
  invoke<CanonicalTrack>("resolve_canonical_candidates", {
    track,
    fallbackArtist,
  });

export const probeDownloadStreams = (
  artist: string,
  album: string,
  tracks: ExpectedTorrentTrackInput[],
  onMatch: (match: { index: number; candidates: AudioCandidate[] }) => void,
): Promise<void> =>
  invoke("probe_download_streams", { artist, album, tracks, progress: new Channel(onMatch) });

export const probeUpgradeCandidates = (
  songIds: number[],
): Promise<Array<{
  index: number;
  title: string;
  artists: string[];
  track_number: number | null;
  disc_number?: number | null;
  url: string;
  available: boolean;
  note: string | null;
  duration_ms?: number | null;
  candidates?: import("@/shared/types/canonical.ts").AudioCandidate[];
  selected_candidate_idx?: number;
}>> =>
  invoke("probe_upgrade_candidates", { songIds });

export const prewarmAlbumPreviews = (
  items: Array<[string, string]>,
): Promise<void> => invoke("prewarm_album_previews", { items });


