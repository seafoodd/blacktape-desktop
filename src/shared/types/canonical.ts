export type AudioQualityTier =
  | "low"
  | "standard_lossy"
  | "high_lossy"
  | "lossless";

export type AudioSource =
  | {
      kind: "http_stream";
      platform: string;
      url: string;
      format_id?: string;
      direct_stream_url?: string;
      expires_at?: number;
    }
  | {
      kind: "torrent";
      info_hash: string;
      magnet_uri: string;
      file_index: number;
      file_path: string;
      file_size: number;
      seeders: number;
    }
  | {
      kind: "local_file";
      path: string;
    };

export interface AudioCandidate {
  source: AudioSource;
  quality_tier: AudioQualityTier;
  codec: string;
  bitrate_kbps?: number;
  sample_rate_hz?: number;
  duration_ms: number;
  match_confidence: number;
  is_strict_match: boolean;
  display_label: string;
  matched_title?: string | null;
  matched_artist?: string | null;
}

export interface CanonicalTrack {
  track_number: number;
  disc_number: number;
  title: string;
  artists: string[];
  duration_ms: number;
  isrc?: string;
  candidates: AudioCandidate[];
  selected_candidate_idx: number;
}

export interface CanonicalDisc {
  disc_number: number;
  title?: string;
  tracks: CanonicalTrack[];
}

export interface CanonicalRelease {
  id: string;
  mbid?: string;
  release_group_id?: string;
  title: string;
  album_artist: string;
  is_various_artists: boolean;
  release_year?: number;
  total_tracks: number;
  total_discs: number;
  discs: CanonicalDisc[];
  cover_url?: string;
  metadata_source: string;
}

export interface CanonicalReleaseSummary {
  id: string;
  mbid?: string;
  title: string;
  artist: string;
  year?: number;
  track_count: number;
  cover_url?: string;
  metadata_source: string;
}

export interface ReleaseEditionOption {
  id: string;
  title: string;
  edition_label: string;
  year: number | null;
  track_count: number;
  disc_count: number;
  format: string | null;
  is_default: boolean;
  is_alternate?: boolean;
  source_kind?: string | null;
  url?: string | null;
}

export function getQualityTierBadge(tier: AudioQualityTier): {
  label: string;
  colorClass: string;
} {
  switch (tier) {
    case "lossless":
      return { label: "FLAC · Lossless", colorClass: "text-emerald-400 border-emerald-500/40 bg-emerald-500/10" };
    case "high_lossy":
      return { label: "High (Opus/AAC)", colorClass: "text-cyan-400 border-cyan-500/40 bg-cyan-500/10" };
    case "standard_lossy":
      return { label: "Standard (128k)", colorClass: "text-amber-400 border-amber-500/40 bg-amber-500/10" };
    case "low":
      return { label: "Low (<128k)", colorClass: "text-rose-400 border-rose-500/40 bg-rose-500/10" };
  }
}
