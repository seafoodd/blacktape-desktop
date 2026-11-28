use super::persist_complete_release;
use crate::{
    platforms::musicbrainz::MusicBrainzClient,
    metadata::types::{AudioCandidate, AudioQualityTier, AudioSource, CanonicalDisc, CanonicalRelease, CanonicalTrack},
};

fn release(id: &str, with_candidate: bool) -> CanonicalRelease {
    let candidates = if with_candidate {
        vec![AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "youtube".into(),
                url: "https://youtube.com/watch?v=fixture".into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: AudioQualityTier::HighLossy,
            codec: "Opus".into(),
            bitrate_kbps: Some(160),
            sample_rate_hz: Some(48_000),
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: "YouTube Music (Opus)".into(),
            matched_title: Some("A track".into()),
            matched_artist: Some("An artist".into()),
        }]
    } else {
        Vec::new()
    };

    CanonicalRelease {
        id: id.into(),
        mbid: Some(id.into()),
        release_group_id: None,
        title: "An album".into(),
        album_artist: "An artist".into(),
        is_various_artists: false,
        release_year: Some(2020),
        total_tracks: 1,
        total_discs: 1,
        discs: vec![CanonicalDisc {
            disc_number: 1,
            title: None,
            tracks: vec![CanonicalTrack {
                track_number: 1,
                disc_number: 1,
                title: "A track".into(),
                artists: vec!["An artist".into()],
                duration_ms: 180_000,
                isrc: None,
                candidates,
                selected_candidate_idx: 0,
            }],
        }],
        cover_url: None,
        metadata_source: "musicbrainz".into(),
    }
}

#[tokio::test]
async fn incomplete_release_is_not_persisted_even_when_it_has_candidates() {
    let directory = tempfile::tempdir().expect("temporary cache directory");
    let client = MusicBrainzClient::new(Some(directory.path().to_path_buf()));
    let release = release("incomplete-with-candidates", true);

    persist_complete_release(&client, &release, false).await;

    assert!(client.read_cache("release_v7:incomplete-with-candidates").await.is_none());
}

#[tokio::test]
async fn incomplete_empty_release_is_not_persisted() {
    let directory = tempfile::tempdir().expect("temporary cache directory");
    let client = MusicBrainzClient::new(Some(directory.path().to_path_buf()));
    let release = release("incomplete-empty", false);

    persist_complete_release(&client, &release, false).await;

    assert!(client.read_cache("release_v7:incomplete-empty").await.is_none());
}

#[tokio::test]
async fn complete_empty_release_is_persisted_as_a_legitimate_result() {
    let directory = tempfile::tempdir().expect("temporary cache directory");
    let client = MusicBrainzClient::new(Some(directory.path().to_path_buf()));
    let release = release("complete-empty", false);

    persist_complete_release(&client, &release, true).await;

    let saved = client.read_cache("release_v7:complete-empty").await
        .expect("completed empty release should be cached");
    let decoded: CanonicalRelease = serde_json::from_str(&saved).expect("cached release should be valid JSON");
    assert_eq!(decoded, release);
    assert!(decoded.all_tracks().iter().all(|track| track.candidates.is_empty()));
}
