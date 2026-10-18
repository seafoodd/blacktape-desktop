export type QualityTier = "low" | "standard" | "high" | "lossless";

export type Platform = "youtube" | "bandcamp" | "local";

export type Song = {
  id: number;
  path: string;
  title: string;
  artists: string[];
  album_artist: string;
  album: string;
  duration_ms: number;
  track_number?: number | null;
  genre?: string | null;
  release_year?: number | null;
  cover_url?: string | null;
  external_cover_url?: string | null;
  lyrics?: string | null;
  lyrics_source?: string | null;
  source: Platform;
  source_url?: string | null;
  source_item_id?: string | null;
  canonical_track_slug: string;
  canonical_album_slug: string;
  quality_tier: QualityTier;
};

export type ArtistSummary = {
  name: string;
  album_count: number;
  cover_url?: string | null;
};

export type Album = {
  title: string;
  album_artist: string;
  cover_url: string | null;
  songs: Song[];
};

export type Playlist = {
  id: number;
  name: string;
  description: string;
  cover_url: string;
  date_added: string;
  date_modified: string;
  track_count: number;
  total_duration_ms: number;
  preview_covers: string;
};

export enum RepeatMode {
  Off = "Off",
  Queue = "Queue",
  Track = "Track",
}
