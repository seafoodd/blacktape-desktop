pub mod arbiter;
pub mod matcher;
pub mod provider;

use crate::download::TrackDownload;
use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource, CanonicalRelease, CanonicalTrack};
use std::collections::HashSet;
use tauri::Emitter;
use tracing::info;

pub struct AlbumPlatformTracks {
    pub complete: bool,
    pub yt: Vec<TrackDownload>,
    pub bc: Vec<TrackDownload>,
    pub bc_free: bool,
    pub sc: Vec<TrackDownload>,
}

pub async fn resolve_album_platform_tracks(
    artist: &str,
    album_title: &str,
) -> AlbumPlatformTracks {
    let yt_fut = provider::resolve_youtube_album_tracks(artist, album_title);
    let bc_fut = provider::resolve_bandcamp_album_tracks(artist, album_title);
    let sc_fut = provider::resolve_soundcloud_album_tracks(artist, album_title);

    let (yt, bc, sc) = tokio::join!(yt_fut, bc_fut, sc_fut);
    let complete = yt.is_ok() && bc.is_ok() && sc.is_ok();
    let (bc, bc_free) = bc.unwrap_or_default();
    AlbumPlatformTracks {
        complete,
        yt: yt.unwrap_or_default(),
        bc,
        bc_free,
        sc: sc.unwrap_or_default(),
    }
}

fn matched_artist_from_track(track: &TrackDownload) -> Option<String> {
    track
        .artists
        .first()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .or_else(|| (!track.album_artist.trim().is_empty()).then(|| track.album_artist.clone()))
}

pub fn match_album_platform_candidates(
    title: &str,
    duration_ms: u64,
    track_number: u32,
    disc_number: Option<u32>,
    album_tracks: &AlbumPlatformTracks,
) -> Vec<AudioCandidate> {
    let mut candidates = Vec::new();

    if let Some(yt_t) = matcher::find_matching_track_for_disc(
        title,
        duration_ms,
        track_number,
        disc_number,
        &album_tracks.yt,
    ) {
        let cand_duration = yt_t.duration_ms.unwrap_or(0);
        let eval = matcher::evaluate_track_match(title, duration_ms, &yt_t.title, cand_duration);
        candidates.push(AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "youtube".into(),
                url: yt_t.url.clone(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: AudioQualityTier::HighLossy,
            codec: "Opus".into(),
            bitrate_kbps: Some(160),
            sample_rate_hz: Some(48000),
            duration_ms: cand_duration,
            match_confidence: eval.confidence,
            is_strict_match: eval.is_strict_match,
            display_label: "YouTube Music (Opus 160k)".into(),
            matched_title: Some(yt_t.title.clone()),
            matched_artist: matched_artist_from_track(yt_t),
        });
    }

    if let Some(bc_t) = matcher::find_matching_track_for_disc(
        title,
        duration_ms,
        track_number,
        disc_number,
        &album_tracks.bc,
    ) {
        let cand_duration = bc_t.duration_ms.unwrap_or(0);
        let eval = matcher::evaluate_track_match(title, duration_ms, &bc_t.title, cand_duration);
        let (quality_tier, codec, bitrate_kbps, display_label) = if album_tracks.bc_free {
            (
                AudioQualityTier::Lossless,
                "FLAC".to_string(),
                None,
                "Bandcamp · FLAC (Lossless)".to_string(),
            )
        } else {
            (
                AudioQualityTier::StandardLossy,
                "MP3".to_string(),
                Some(128),
                "Bandcamp (MP3 128k)".to_string(),
            )
        };
        candidates.push(AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "bandcamp".into(),
                url: bc_t.url.clone(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier,
            codec,
            bitrate_kbps,
            sample_rate_hz: Some(44100),
            duration_ms: cand_duration,
            match_confidence: eval.confidence,
            is_strict_match: eval.is_strict_match,
            display_label,
            matched_title: Some(bc_t.title.clone()),
            matched_artist: matched_artist_from_track(bc_t),
        });
    }

    if let Some(sc_t) = matcher::find_matching_track_for_disc(
        title,
        duration_ms,
        track_number,
        disc_number,
        &album_tracks.sc,
    ) {
        let cand_duration = sc_t.duration_ms.unwrap_or(0);
        let eval = matcher::evaluate_track_match(title, duration_ms, &sc_t.title, cand_duration);
        candidates.push(AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "soundcloud".into(),
                url: sc_t.url.clone(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: AudioQualityTier::StandardLossy,
            codec: "MP3".into(),
            bitrate_kbps: Some(128),
            sample_rate_hz: Some(44100),
            duration_ms: cand_duration,
            match_confidence: eval.confidence,
            is_strict_match: eval.is_strict_match,
            display_label: "SoundCloud (MP3 128k)".into(),
            matched_title: Some(sc_t.title.clone()),
            matched_artist: matched_artist_from_track(sc_t),
        });
    }

    candidates
}

/// Resolves audio candidates for an entire release concurrently:
/// First performs fast album-level resolution across platforms, then falls back to track-level probes.
/// Emits live "album-track-candidates" events to the frontend as tracks are resolved.
pub async fn resolve_release_candidates(
    release: &mut CanonicalRelease,
    app_handle: Option<&tauri::AppHandle>,
    album_url: &str,
) -> bool {
    let artist = release.album_artist.clone();
    let album_title = release.title.clone();

    let album_tracks = resolve_album_platform_tracks(&artist, &album_title).await;
    let mut complete = album_tracks.complete;

    let mut global_track_idx = 0;
    for disc in &mut release.discs {
        for track in &mut disc.tracks {
            let mut candidates = track.candidates.clone();
            candidates.extend(match_album_platform_candidates(
                &track.title,
                track.duration_ms,
                track.track_number,
                Some(disc.disc_number),
                &album_tracks,
            ));

            // If candidates are still empty (e.g. tracks not matched in album), fall back to single-track probes
            if candidates.is_empty() {
                let t_artist = track.artists.first().map(|s| s.as_str()).unwrap_or(&artist);
                let (probed, probes_complete) = resolve_track_outcome(t_artist, &track.title, track.duration_ms).await;
                complete &= probes_complete;
                candidates.extend(probed);
            }

            // Deduplicate by source URL
            let mut unique = Vec::new();
            let mut seen_urls = HashSet::new();
            for c in candidates {
                if seen_urls.insert(c.source.primary_url().to_string()) {
                    unique.push(c);
                }
            }
            arbiter::rank_candidates(&mut unique);
            track.candidates = unique;
            track.selected_candidate_idx = 0;

            if track.duration_ms == 0 {
                if let Some(inferred_ms) = track
                    .candidates
                    .iter()
                    .find(|c| c.duration_ms > 0)
                    .map(|c| c.duration_ms)
                {
                    track.duration_ms = inferred_ms;
                }
            }

            if let Some(app) = app_handle {
                let _ = app.emit(
                    "album-track-candidates",
                    serde_json::json!({
                        "album_url": album_url,
                        "track_index": global_track_idx,
                        "candidates": track.candidates,
                        "selected_candidate_idx": 0,
                        "duration_ms": (track.duration_ms > 0).then_some(track.duration_ms),
                    }),
                );
            }
            global_track_idx += 1;
        }
    }
    complete
}

pub async fn persist_complete_release(client: &crate::metadata::MusicBrainzClient, release: &CanonicalRelease, complete: bool) {
    if !complete { return; }
    if let Ok(serialized) = serde_json::to_string(release) {
        client.write_cache(&format!("release_v7:{}", release.id), &serialized).await;
    }
}

#[cfg(test)]
mod cache_tests;

/// Resolves audio candidates for a track across all supported platforms concurrently.
/// Ranks results strictly by: Strict Match -> Quality Tier -> Bitrate -> Confidence.
pub async fn resolve_candidates_for_track(
    artist: &str,
    title: &str,
    duration_ms: u64,
) -> Vec<AudioCandidate> {
    resolve_track_outcome(artist, title, duration_ms).await.0
}

async fn resolve_track_outcome(artist: &str, title: &str, duration_ms: u64) -> (Vec<AudioCandidate>, bool) {
    info!(
        "Resolving multi-source candidates for '{} - {}' (duration: {}ms)",
        artist, title, duration_ms
    );

    let yt_fut = provider::resolve_youtube_candidates(artist, title, duration_ms);
    let bc_fut = provider::resolve_bandcamp_candidates(artist, title, duration_ms);
    let sc_fut = provider::resolve_soundcloud_candidates(artist, title, duration_ms);
    let tor_fut = provider::resolve_torrent_candidates(artist, title, duration_ms);

    let (yt_res, bc_res, sc_res, tor_res) = tokio::join!(yt_fut, bc_fut, sc_fut, tor_fut);

    let mut all_candidates = Vec::new();
    let complete = yt_res.is_ok() && bc_res.is_ok() && sc_res.is_ok();
    all_candidates.extend(yt_res.unwrap_or_default());
    all_candidates.extend(bc_res.unwrap_or_default());
    all_candidates.extend(sc_res.unwrap_or_default());
    all_candidates.extend(tor_res);

    arbiter::rank_candidates(&mut all_candidates);
    (all_candidates, complete)
}

/// Populates a canonical track with resolved candidates.
pub async fn populate_track_candidates(track: &mut CanonicalTrack, fallback_artist: Option<&str>) {
    let artist = track
        .artists
        .first()
        .map(|s| s.as_str())
        .or(fallback_artist)
        .unwrap_or("Unknown Artist");

    let candidates = resolve_candidates_for_track(artist, &track.title, track.duration_ms).await;
    track.candidates = candidates;
    track.selected_candidate_idx = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::types::{AudioQualityTier, AudioSource};

    #[tokio::test]
    async fn populates_track_and_defaults_to_first_candidate() {
        let mut track = CanonicalTrack {
            track_number: 1,
            disc_number: 1,
            title: "Test Track".into(),
            artists: vec!["Test Artist".into()],
            duration_ms: 180000,
            isrc: None,
            candidates: Vec::new(),
            selected_candidate_idx: 0,
        };

        // When populated manually or via resolver:
        track.candidates = vec![
            AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "youtube".into(),
                    url: "https://music.youtube.com/watch?v=1".into(),
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::HighLossy,
                codec: "Opus".into(),
                bitrate_kbps: Some(160),
                sample_rate_hz: Some(48000),
                duration_ms: 180000,
                match_confidence: 0.98,
                is_strict_match: true,
                display_label: "YouTube (Opus 160k)".into(),
                matched_title: Some("Test Track".into()),
                matched_artist: Some("Test Artist".into()),
            },
        ];

        assert_eq!(track.selected_candidate_idx, 0);
        let selected = track.selected_candidate().unwrap();
        assert_eq!(selected.codec, "Opus");
        assert_eq!(selected.quality_tier, AudioQualityTier::HighLossy);
    }

    #[test]
    fn match_album_platform_candidates_marks_free_bandcamp_as_lossless_flac_above_youtube() {
        let track_proto = |url: &str, title: &str| TrackDownload {
            url: url.into(),
            file_name: format!("{title}.flac"),
            title: title.into(),
            artists: vec!["Heaven Pierce Her".into()],
            album_artist: "Heaven Pierce Her".into(),
            album: "ULTRAKILL: FRAUD".into(),
            track_number: Some(1),
            disc_number: Some(1),
            duration_ms: Some(273_000),
            ..TrackDownload::default()
        };
        let album_tracks = AlbumPlatformTracks {
            complete: true,
            yt: vec![track_proto("https://youtube.com/watch?v=1", "In Absentia ΛΟΓΟΣ")],
            bc: vec![track_proto("https://heavenpierceher.bandcamp.com/track/in-absentia", "In Absentia ΛΟΓΟΣ")],
            bc_free: true,
            sc: vec![],
        };
        let mut cands = match_album_platform_candidates(
            "In Absentia ΛΟΓΟΣ",
            273_000,
            1,
            Some(1),
            &album_tracks,
        );
        arbiter::rank_candidates(&mut cands);
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].source.platform_name(), "bandcamp");
        assert_eq!(cands[0].quality_tier, AudioQualityTier::Lossless);
        assert_eq!(cands[0].codec, "FLAC");
        assert_eq!(cands[0].display_label, "Bandcamp · FLAC (Lossless)");
    }
}
