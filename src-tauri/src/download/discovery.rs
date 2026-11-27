use std::collections::HashMap;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tokio::task::JoinSet;

use super::TrackDownload;
use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
use crate::resolver::{self, AlbumPlatformTracks};
use crate::torrent::matcher::{ExpectedTrackInput, TorrentResolveEvent};
use crate::types::Platform;

const SEARCH_WINDOW: Duration = Duration::from_secs(7);

#[derive(serde::Serialize)]
pub(crate) struct StreamMatch {
    index: usize,
    candidates: Vec<AudioCandidate>,
}

fn add_candidate(candidates: &mut Vec<AudioCandidate>, candidate: AudioCandidate) {
    if candidate.is_strict_match
        && !candidates.iter().any(|old| old.source == candidate.source && old.quality_tier == candidate.quality_tier)
    {
        candidates.push(candidate);
    }
}

async fn stream_matches(
    artist: &str,
    album: &str,
    tracks: &[ExpectedTrackInput],
    mut publish: impl FnMut(StreamMatch),
) {
    let mut tasks = JoinSet::new();
    for platform in [Platform::Youtube, Platform::Bandcamp, Platform::Soundcloud] {
        if tracks.len() > 3 && !album.is_empty() {
            let (artist, album, tracks) = (artist.to_owned(), album.to_owned(), tracks.to_vec());
            tasks.spawn(async move {
                let mut sources = AlbumPlatformTracks { complete: false, yt: vec![], bc: vec![], bc_free: false, sc: vec![] };
                let (resolved, free) = crate::platforms::album_tracks(platform, &artist, &album).await.unwrap_or_default();
                match platform {
                    Platform::Youtube => sources.yt = resolved,
                    Platform::Bandcamp => { sources.bc = resolved; sources.bc_free = free; }
                    Platform::Soundcloud => sources.sc = resolved,
                    Platform::Local => return Vec::new(),
                }
                tracks.into_iter().map(|track| {
                    let candidates = resolver::match_album_platform_candidates(
                        &track.title, track.duration_ms.unwrap_or(0), track.track_number.unwrap_or(0),
                        track.disc_number, &sources,
                    );
                    StreamMatch { index: track.index, candidates }
                }).collect::<Vec<_>>()
            });
            continue;
        }
        for track in tracks {
            let artist = track.artist.as_deref().filter(|name| !name.is_empty()).unwrap_or(artist).to_owned();
            let track = track.clone();
            tasks.spawn(async move {
                let length = track.duration_ms.unwrap_or(0);
                let candidates = crate::platforms::track_candidates(platform, &artist, &track.title, length).await.unwrap_or_default();
                vec![StreamMatch { index: track.index, candidates }]
            });
        }
    }
    while let Some(result) = tasks.join_next().await {
        for mut item in result.unwrap_or_default() {
            item.candidates.retain(|candidate| candidate.is_strict_match);
            if !item.candidates.is_empty() {
                publish(item);
            }
        }
    }
}

#[tauri::command]
pub(crate) async fn probe_download_streams(
    window: WebviewWindow,
    artist: String,
    album: String,
    tracks: Vec<ExpectedTrackInput>,
    progress: tauri::ipc::Channel<StreamMatch>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Download discovery is only available in the main window.".into());
    }
    let _ = tokio::time::timeout(SEARCH_WINDOW, stream_matches(&artist, &album, &tracks, |item| {
        let _ = progress.send(item);
    })).await;
    Ok(())
}

fn expected_track(index: usize, track: &TrackDownload) -> ExpectedTrackInput {
    ExpectedTrackInput {
        index,
        title: track.title.clone(),
        artist: track.artists.first().cloned(),
        duration_ms: track.duration_ms,
        track_number: track.track_number.and_then(|number| u32::try_from(number).ok()),
        disc_number: track.disc_number.and_then(|number| u32::try_from(number).ok()),
    }
}

fn linked_stream(track: &TrackDownload) -> Option<AudioCandidate> {
    use super::ytdlp::{download_site, DownloadSite};
    let (name, quality_tier, codec, bitrate_kbps) = match download_site(&track.url) {
        DownloadSite::Youtube => ("youtube", AudioQualityTier::HighLossy, "Opus", 160),
        DownloadSite::Bandcamp => ("bandcamp", AudioQualityTier::StandardLossy, "MP3", 128),
        DownloadSite::Soundcloud => ("soundcloud", AudioQualityTier::StandardLossy, "MP3", 128),
        DownloadSite::Other => return None,
    };
    Some(AudioCandidate {
        source: AudioSource::HttpStream {
            platform: name.into(), url: track.url.clone(),
            format_id: None, direct_stream_url: None, expires_at: None,
        },
        quality_tier, codec: codec.into(), bitrate_kbps: Some(bitrate_kbps), sample_rate_hz: None,
        duration_ms: track.duration_ms.unwrap_or(0), match_confidence: 1., is_strict_match: true,
        display_label: name.into(), matched_title: Some(track.title.clone()), matched_artist: track.artists.first().cloned(),
    })
}

fn apply_automatic_sources(track: &mut TrackDownload, mut candidates: Vec<AudioCandidate>, format: &str) {
    candidates.retain(|candidate| candidate.is_strict_match);
    if candidates.is_empty() {
        return;
    }
    if let Some(linked) = linked_stream(track) {
        // A successful lookup of this URL takes precedence over its platform's estimated quality.
        let linked_key = crate::library_index::normalize::normalize_url_key(&track.url);
        if !candidates.iter().any(|candidate| matches!(&candidate.source, AudioSource::HttpStream { .. })
            && linked_key.as_ref().is_some_and(|key| crate::library_index::normalize::normalize_url_key(
                candidate.source.primary_url()).as_ref() == Some(key)))
        {
            candidates.push(linked);
        }
    }
    resolver::arbiter::rank_candidates(&mut candidates);
    let original = track.url.clone();
    super::album::apply_candidate_to_track(track, &candidates, Some(format), 0);
    if original != track.url && !original.is_empty() && !track.fallback_urls.contains(&original) {
        track.fallback_urls.push(original);
    }
}

pub(super) async fn discover_automatic_sources(
    app: &AppHandle,
    task_id: &str,
    artist: &str,
    album: &str,
    tracks: &mut [TrackDownload],
    supplied: impl Fn(usize, &TrackDownload) -> bool,
    format: &str,
) {
    let expected = tracks.iter().enumerate()
        .filter(|(index, track)| !supplied(*index, track))
        .map(|(index, track)| expected_track(index, track)).collect::<Vec<_>>();
    if expected.is_empty() {
        return;
    }
    let message = "Finding audio sources…";
    if let Some(registry) = app.try_state::<super::TaskRegistry>() {
        if let Some((current, total)) = registry.set_task_message(task_id, message).await {
            let _ = app.emit("download-task-progress", super::ProgressEvent {
                task_id: task_id.into(), current, total, track_title: message.into(),
            });
        }
    }
    let mut streams = HashMap::<usize, Vec<AudioCandidate>>::new();
    let torrents = std::sync::Mutex::new(Vec::new());
    let torrent_search = async {
        let search_title = if album.is_empty() { &expected[0].title } else { album };
        let _ = crate::torrent::commands::resolve_enabled_torrents(app, artist, search_title, &expected, |event| {
            if let TorrentResolveEvent::Verified { resolutions } | TorrentResolveEvent::Resolved { resolutions, .. } = event {
                if let Ok(mut results) = torrents.lock() {
                    *results = resolutions;
                }
            }
        }).await;
    };
    let _ = tokio::time::timeout(SEARCH_WINDOW, async {
        tokio::join!(stream_matches(artist, album, &expected, |result| {
            let candidates = streams.entry(result.index).or_default();
            for candidate in result.candidates {
                add_candidate(candidates, candidate);
            }
        }), torrent_search);
    }).await;
    for release in torrents.into_inner().unwrap_or_default() {
        for item in release.tracks {
            add_candidate(streams.entry(item.expected_index).or_default(), item.candidate);
        }
    }
    for (index, candidates) in streams {
        if let Some(track) = tracks.get_mut(index) {
            apply_automatic_sources(track, candidates, format);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(url: &str) -> TrackDownload {
        TrackDownload {
            url: url.into(), title: "Sunshine (Mirage)".into(),
            artists: vec!["Heaven Pierce Her".into()], duration_ms: Some(180_000),
            ..TrackDownload::default()
        }
    }

    #[test]
    fn quick_download_picks_the_configured_tier_across_platforms() {
        let original = "https://www.youtube.com/watch?v=linked";
        let bandcamp = "https://heavenpierceher.bandcamp.com/track/sunshine";
        let soundcloud = "https://soundcloud.com/heavenpierceher/sunshine";
        let mut lossless = linked_stream(&track(bandcamp)).unwrap();
        lossless.quality_tier = AudioQualityTier::Lossless;
        let standard = linked_stream(&track(soundcloud)).unwrap();
        for (format, wanted) in [("best", bandcamp), ("high", original), ("standard", soundcloud)] {
            let mut picked = track(original);
            apply_automatic_sources(&mut picked, vec![standard.clone(), lossless.clone()], format);
            assert_eq!(picked.url, wanted);
        }
    }

    #[test]
    fn automatic_best_uses_verified_torrent_and_rejects_a_near_match() {
        let original = "https://www.youtube.com/watch?v=linked";
        let mut torrent = linked_stream(&track(original)).unwrap();
        torrent.quality_tier = AudioQualityTier::Lossless;
        torrent.source = AudioSource::Torrent {
            info_hash: "a".repeat(40), magnet_uri: format!("magnet:?xt=urn:btih:{}", "a".repeat(40)),
            file_index: 3, file_path: "04 Sunshine (Mirage).flac".into(), file_size: 20_000_000, seeders: 2,
        };
        let mut picked = track(original);
        torrent.is_strict_match = false;
        apply_automatic_sources(&mut picked, vec![torrent.clone()], "best");
        assert!(picked.torrent_target.is_none());
        assert_eq!(picked.url, original);
        torrent.is_strict_match = true;
        apply_automatic_sources(&mut picked, vec![torrent], "best");
        assert_eq!(picked.torrent_target.unwrap().file_index, 3);
        assert_eq!(picked.fallback_urls, [original]);
    }

    #[test]
    fn missing_preferred_tier_preserves_a_known_source() {
        let original = "https://www.youtube.com/watch?v=linked";
        let mut picked = track(original);
        apply_automatic_sources(&mut picked, vec![], "standard");
        assert_eq!(picked.url, original);
        let lossless_url = "https://heavenpierceher.bandcamp.com/track/sunshine";
        let mut lossless = linked_stream(&track(lossless_url)).unwrap();
        lossless.quality_tier = AudioQualityTier::Lossless;
        apply_automatic_sources(&mut picked, vec![lossless], "high");
        assert_eq!(picked.url, original);
        assert!(linked_stream(&track("https://example.test/youtube.com/watch?v=linked")).is_none());
    }
}
