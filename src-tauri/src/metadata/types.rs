use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Quality tiers ranked strictly from highest fidelity to lowest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioQualityTier {
    Low = 0,
    StandardLossy = 1,
    HighLossy = 2,
    Lossless = 3,
}

impl AudioQualityTier {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Lossless => "Lossless (FLAC)",
            Self::HighLossy => "High (Opus/AAC/320k)",
            Self::StandardLossy => "Standard (128k/160k)",
            Self::Low => "Low (<128k)",
        }
    }
}

/// Polymorphic audio source definition. Supports HTTP streams now and BitTorrent swarms in Milestone 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioSource {
    HttpStream {
        platform: String, // "youtube", "bandcamp", "soundcloud"
        url: String,
        format_id: Option<String>,
        direct_stream_url: Option<String>,
        expires_at: Option<u64>,
    },
    Torrent {
        info_hash: String,
        magnet_uri: String,
        file_index: usize,
        file_path: String,
        file_size: u64,
        seeders: u32,
    },
    LocalFile {
        path: PathBuf,
    },
}

impl AudioSource {
    pub fn platform_name(&self) -> &str {
        match self {
            AudioSource::HttpStream { platform, .. } => platform.as_str(),
            AudioSource::Torrent { .. } => "torrent",
            AudioSource::LocalFile { .. } => "local",
        }
    }

    pub fn primary_url(&self) -> &str {
        match self {
            AudioSource::HttpStream { url, .. } => url.as_str(),
            AudioSource::Torrent { magnet_uri, .. } => magnet_uri.as_str(),
            AudioSource::LocalFile { path } => path.to_str().unwrap_or_default(),
        }
    }
}

/// A candidate audio stream/file discovered by an audio provider for a canonical track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioCandidate {
    pub source: AudioSource,
    pub quality_tier: AudioQualityTier,
    pub codec: String,             // "FLAC", "Opus", "AAC", "MP3"
    pub bitrate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub duration_ms: u64,
    pub match_confidence: f32,     // 0.0 to 1.0 (title + duration delta)
    pub is_strict_match: bool,     // duration delta <= 4s, no version/title mismatch
    pub display_label: String,     // e.g. "FLAC (Lossless) [Bandcamp]"
    #[serde(default)]
    pub matched_title: Option<String>,
    #[serde(default)]
    pub matched_artist: Option<String>,
}

/// A track in a canonical release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalTrack {
    pub track_number: u32,
    pub disc_number: u32,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: u64,
    pub isrc: Option<String>,
    pub candidates: Vec<AudioCandidate>,
    pub selected_candidate_idx: usize,
}

impl CanonicalTrack {
    pub fn selected_candidate(&self) -> Option<&AudioCandidate> {
        self.candidates.get(self.selected_candidate_idx)
    }

    pub fn selected_candidate_mut(&mut self) -> Option<&mut AudioCandidate> {
        self.candidates.get_mut(self.selected_candidate_idx)
    }
}

/// A disc container grouping tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalDisc {
    pub disc_number: u32,
    pub title: Option<String>,
    pub tracks: Vec<CanonicalTrack>,
}

/// The single source of truth for an album release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalRelease {
    pub id: String, // mbid or synthesized id
    pub mbid: Option<String>,
    #[serde(default)]
    pub release_group_id: Option<String>,
    pub title: String,
    pub album_artist: String,
    pub is_various_artists: bool,
    pub release_year: Option<u32>,
    pub total_tracks: usize,
    pub total_discs: usize,
    pub discs: Vec<CanonicalDisc>,
    pub cover_url: Option<String>,
    pub metadata_source: String, // "musicbrainz" or "platform_synthesized"
}

impl CanonicalRelease {
    pub fn all_tracks(&self) -> Vec<&CanonicalTrack> {
        self.discs.iter().flat_map(|d| d.tracks.iter()).collect()
    }

    pub fn all_tracks_mut(&mut self) -> Vec<&mut CanonicalTrack> {
        self.discs.iter_mut().flat_map(|d| d.tracks.iter_mut()).collect()
    }
}

/// Compact summary of a canonical release used in search results and dropdowns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalReleaseSummary {
    pub id: String, // mbid or synth id
    pub mbid: Option<String>,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub track_count: usize,
    pub cover_url: Option<String>,
    pub metadata_source: String,
}

/// Distinct edition within a MusicBrainz release-group shown in the album release picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseEditionOption {
    pub id: String,
    pub title: String,
    pub edition_label: String,
    pub year: Option<i32>,
    pub track_count: usize,
    pub disc_count: usize,
    pub format: Option<String>,
    pub is_default: bool,
    #[serde(default)]
    pub is_alternate: bool,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}
