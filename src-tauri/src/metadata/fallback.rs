use crate::download::album_preview::AlbumPreview;
use crate::metadata::types::{CanonicalDisc, CanonicalRelease, CanonicalTrack};
use std::collections::BTreeMap;

/// Synthesizes a CanonicalRelease from a platform AlbumPreview (Option B fallback).
pub fn synthesize_canonical_from_preview(preview: AlbumPreview) -> CanonicalRelease {
    let preview_artist = preview.artist.trim();
    let is_various_artists_explicit = preview_artist.eq_ignore_ascii_case("Various Artists");

    // Check if tracks have diverse artists to detect compilations automatically
    let mut distinct_track_artists = std::collections::HashSet::new();
    for track in &preview.tracks {
        for a in &track.artists {
            let trimmed = a.trim();
            if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("Unknown Artist") {
                distinct_track_artists.insert(trimmed.to_lowercase());
            }
        }
    }

    let is_va = is_various_artists_explicit || distinct_track_artists.len() >= 3;
    let album_artist = if is_va {
        "Various Artists".to_string()
    } else if !preview_artist.is_empty() {
        preview_artist.to_string()
    } else {
        "Unknown Artist".to_string()
    };

    // Group tracks by disc_number (defaulting to disc 1)
    let mut discs_map: BTreeMap<u32, Vec<CanonicalTrack>> = BTreeMap::new();

    for (idx, track) in preview.tracks.into_iter().enumerate() {
        let disc_num = track.disc_number.map(|d| d.max(1) as u32).unwrap_or(1);
        let track_num = track.track_number.map(|n| n.max(1) as u32).unwrap_or((idx + 1) as u32);

        let artists = if !track.artists.is_empty() {
            track.artists
        } else {
            vec![album_artist.clone()]
        };

        let duration_ms = track.duration_ms.unwrap_or(0);

        let mut initial_candidates = Vec::new();
        if track.available && !track.url.is_empty() {
            let (codec, bitrate, sample_rate, display_label, tier) = match preview.platform {
                crate::types::Platform::Bandcamp if preview.is_free_download => (
                    "FLAC".to_string(),
                    None,
                    Some(44100),
                    "Bandcamp · FLAC (Lossless)".to_string(),
                    crate::metadata::types::AudioQualityTier::Lossless,
                ),
                crate::types::Platform::Bandcamp => (
                    "MP3".to_string(),
                    Some(128),
                    Some(44100),
                    "Bandcamp · MP3 128 kbps".to_string(),
                    crate::metadata::types::AudioQualityTier::StandardLossy,
                ),
                crate::types::Platform::Youtube => (
                    "Opus".to_string(),
                    Some(160),
                    Some(48000),
                    "YouTube · Opus 160 kbps".to_string(),
                    crate::metadata::types::AudioQualityTier::HighLossy,
                ),
                crate::types::Platform::Soundcloud => (
                    "MP3".to_string(),
                    Some(128),
                    Some(44100),
                    "SoundCloud · MP3 128 kbps".to_string(),
                    crate::metadata::types::AudioQualityTier::StandardLossy,
                ),
                crate::types::Platform::Local => (
                    "Local".to_string(),
                    None,
                    None,
                    "Local File".to_string(),
                    crate::metadata::types::AudioQualityTier::StandardLossy,
                ),
            };
            initial_candidates.push(crate::metadata::types::AudioCandidate {
                source: crate::metadata::types::AudioSource::HttpStream {
                    platform: preview.platform.to_string().to_lowercase(),
                    url: track.url.clone(),
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: tier,
                codec,
                bitrate_kbps: bitrate,
                sample_rate_hz: sample_rate,
                duration_ms,
                match_confidence: 1.0,
                is_strict_match: true,
                display_label,
                matched_title: Some(track.title.clone()),
                matched_artist: artists.first().cloned(),
            });
        }

        discs_map.entry(disc_num).or_default().push(CanonicalTrack {
            track_number: track_num,
            disc_number: disc_num,
            title: track.title,
            artists,
            duration_ms,
            isrc: None,
            candidates: initial_candidates,
            selected_candidate_idx: 0,
        });
    }

    let total_tracks: usize = discs_map.values().map(|t| t.len()).sum();
    let total_discs = discs_map.len().max(1);

    let mut discs = Vec::new();
    for (disc_number, tracks) in discs_map {
        discs.push(CanonicalDisc {
            disc_number,
            title: None,
            tracks,
        });
    }

    let release_year = preview.year.and_then(|y| if y > 0 { Some(y as u32) } else { None });
    let synthesized_id = format!("synth:{}:{}", preview.platform, xxhash_rust::xxh3::xxh3_64(preview.url.as_bytes()));

    CanonicalRelease {
        id: synthesized_id,
        mbid: None,
        release_group_id: None,
        title: preview.title,
        album_artist,
        is_various_artists: is_va,
        release_year,
        total_tracks,
        total_discs,
        discs,
        cover_url: preview.cover_url,
        metadata_source: format!("{}_synthesized", preview.platform),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::album_preview::AlbumTrackInfo;
    use crate::types::Platform;

    #[test]
    fn synthesizes_single_artist_album() {
        let preview = AlbumPreview {
            platform: Platform::Youtube,
            url: "https://music.youtube.com/playlist?list=OLAK5uy_test".into(),
            title: "Drukqs".into(),
            artist: "Aphex Twin".into(),
            cover_url: Some("https://cover.test/drukqs.jpg".into()),
            year: Some(2001),
            genres: vec!["Electronic".into()],
            tracks: vec![
                AlbumTrackInfo {
                    index: 0,
                    title: "Jynweythek".into(),
                    artists: vec!["Aphex Twin".into()],
                    track_number: Some(1),
                    disc_number: Some(1),
                    url: "https://music.youtube.com/watch?v=1".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(134000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
                AlbumTrackInfo {
                    index: 1,
                    title: "Vordhosbn".into(),
                    artists: vec!["Aphex Twin".into()],
                    track_number: Some(2),
                    disc_number: Some(1),
                    url: "https://music.youtube.com/watch?v=2".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(282000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
                AlbumTrackInfo {
                    index: 2,
                    title: "54 Cymru Beats".into(),
                    artists: vec!["Aphex Twin".into()],
                    track_number: Some(1),
                    disc_number: Some(2),
                    url: "https://music.youtube.com/watch?v=3".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(360000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
            ],
            total_duration_ms: Some(776000),
            metadata_source: "youtube".into(),
            is_free_download: false,
            release_group_id: None,
            selected_release_id: None,
            candidates_resolved: false,
        };

        let canonical = synthesize_canonical_from_preview(preview);

        assert_eq!(canonical.title, "Drukqs");
        assert_eq!(canonical.album_artist, "Aphex Twin");
        assert!(!canonical.is_various_artists);
        assert_eq!(canonical.release_year, Some(2001));
        assert_eq!(canonical.total_discs, 2);
        assert_eq!(canonical.total_tracks, 3);
        assert_eq!(canonical.discs[0].tracks.len(), 2);
        assert_eq!(canonical.discs[1].tracks.len(), 1);
        assert_eq!(canonical.discs[0].tracks[0].title, "Jynweythek");
        assert_eq!(canonical.discs[1].tracks[0].disc_number, 2);
        assert_eq!(canonical.discs[1].tracks[0].track_number, 1);
        assert!(canonical.metadata_source.contains("youtube"));
    }

    #[test]
    fn detects_various_artists_compilation_from_diverse_tracks() {
        let preview = AlbumPreview {
            platform: Platform::Soundcloud,
            url: "https://soundcloud.com/sets/compilation".into(),
            title: "Summer Vibes 2026".into(),
            artist: "Various Artists".into(),
            cover_url: None,
            year: None,
            genres: vec![],
            tracks: vec![
                AlbumTrackInfo {
                    index: 0,
                    title: "Track 1".into(),
                    artists: vec!["Artist A".into()],
                    track_number: Some(1),
                    disc_number: None,
                    url: "https://soundcloud.com/track1".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(180000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
                AlbumTrackInfo {
                    index: 1,
                    title: "Track 2".into(),
                    artists: vec!["Artist B".into()],
                    track_number: Some(2),
                    disc_number: None,
                    url: "https://soundcloud.com/track2".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(200000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
                AlbumTrackInfo {
                    index: 2,
                    title: "Track 3".into(),
                    artists: vec!["Artist C".into()],
                    track_number: Some(3),
                    disc_number: None,
                    url: "https://soundcloud.com/track3".into(),
                    available: true,
                    note: None,
                    duration_ms: Some(220000),
                    candidates: Vec::new(),
                    selected_candidate_idx: 0,
                },
            ],
            total_duration_ms: Some(600000),
            metadata_source: "soundcloud".into(),
            is_free_download: false,
            release_group_id: None,
            selected_release_id: None,
            candidates_resolved: false,
        };

        let canonical = synthesize_canonical_from_preview(preview);

        assert!(canonical.is_various_artists);
        assert_eq!(canonical.album_artist, "Various Artists");
        assert_eq!(canonical.discs[0].tracks[0].artists, vec!["Artist A"]);
        assert_eq!(canonical.discs[0].tracks[1].artists, vec!["Artist B"]);
    }
}
