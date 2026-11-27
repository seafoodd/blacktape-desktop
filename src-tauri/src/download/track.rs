use super::{bandcamp, soundcloud, youtube};
use crate::download::album::download_album_cover;
use crate::download::types::{AlbumMetaEvent, DownloadResult, ProgressEvent, TrackDownload, TrackMetaItem};
use crate::download::ytdlp;
use crate::types::Platform;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

pub(super) const NO_TRACK_UPGRADE: &str = "No better audio source was found. Your current file was kept.";

fn upgrade_candidate_index(
    candidates: &[crate::metadata::types::AudioCandidate],
    format: Option<&str>,
    selected: usize,
    local_tier: crate::types::QualityTier,
    local_exists: bool,
) -> Result<usize, String> {
    let chosen = crate::download::album::pick_candidate_entry_for_tier(candidates, format, selected);
    if !local_exists {
        return Ok(chosen.map_or(0, |entry| entry.0));
    }
    chosen.filter(|(index, _, _)| crate::download::album::candidate_improves_local(
        local_tier, true, candidates[*index].quality_tier
    )).map(|entry| entry.0).ok_or_else(|| NO_TRACK_UPGRADE.into())
}

pub async fn process_track_url(
    platform: Platform,
    url: &str,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<DownloadResult, String> {
    let _ = app_handle.emit(
        "download-task-progress",
        ProgressEvent {
            task_id: task_id.to_string(),
            current: 0,
            total: 1,
            track_title: "Fetching track metadata...".into(),
        },
    );

    let registry = app_handle.try_state::<crate::download::TaskRegistry>();
    let effective_format = match &registry {
        Some(r) => r.format(task_id).await,
        None => None,
    };
    let task_overrides = match &registry {
        Some(r) => r.overrides(task_id).await,
        None => None,
    };
    let replace_song_id = match &registry {
        Some(r) => r.replace_song_id(task_id).await,
        None => None,
    };

    let (mut track_info, external_cover_url) = if url.starts_with("resolve:") {
        let parsed_id = url
            .strip_prefix("resolve:song:")
            .and_then(|s| s.parse::<i64>().ok())
            .or(replace_song_id);

        let old_song = if let (Some(id), Some(db_state)) = (
            parsed_id,
            app_handle.try_state::<tokio::sync::Mutex<crate::db::db::Database>>(),
        ) {
            let db = db_state.lock().await;
            db.get_song_by_id(id).await.ok().flatten()
        } else {
            None
        };

        let existing_song = old_song.ok_or_else(|| {
            "Multi-source track upgrade requires an existing library song record.".to_string()
        })?;

        let search_artist = existing_song
            .artists
            .first()
            .filter(|a| !a.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| existing_song.album_artist.clone());

        let local_exists = std::path::Path::new(&existing_song.path).exists();
        let supplied = task_overrides.as_ref().and_then(|item| item.tracks.as_ref())
            .and_then(|tracks| tracks.first());
        let mut candidates = supplied.and_then(|item| item.candidates.clone()).unwrap_or_default();
        let selected_source = supplied.and_then(|item| candidates.get(item.selected_candidate_idx.unwrap_or(0)))
            .filter(|candidate| crate::download::album::candidate_improves_local(
                existing_song.quality_tier, local_exists, candidate.quality_tier,
            )).map(|candidate| candidate.source.clone());
        if selected_source.is_none() || effective_format.as_deref().is_some_and(|format| format != "best") {
            for candidate in crate::resolver::resolve_candidates_for_track(
                &search_artist, &existing_song.title, existing_song.duration_ms,
            ).await {
                if !candidates.iter().any(|existing| existing.source == candidate.source) {
                    candidates.push(candidate);
                }
            }
        }
        crate::resolver::arbiter::rank_candidates(&mut candidates);
        let selected = selected_source.and_then(|source| candidates.iter().position(|candidate| candidate.source == source))
            .unwrap_or(0);
        let chosen = upgrade_candidate_index(
            &candidates, effective_format.as_deref(), selected, existing_song.quality_tier, local_exists,
        )?;

        let mut td = crate::download::types::TrackDownload {
            url: String::new(),
            file_name: String::new(),
            title: existing_song.title.clone(),
            artists: existing_song.artists.clone(),
            album_artist: existing_song.album_artist.clone(),
            album: existing_song.album.clone(),
            track_number: existing_song.track_number,
            disc_number: existing_song.disc_number,
            genres: existing_song
                .genre
                .as_ref()
                .map(|g| vec![g.clone()]),
            release_year: existing_song.release_year,
            source_item_id: None,
            duration_ms: Some(existing_song.duration_ms),
            fallback_urls: Vec::new(),
            ..Default::default()
        };

        let _ = crate::download::album::apply_candidate_to_track(
            &mut td,
            &candidates,
            effective_format.as_deref(),
            chosen,
        );

        if let Some(ref orig_url) = existing_song.source_url {
            if orig_url.starts_with("http://") || orig_url.starts_with("https://") {
                if td.url.is_empty() {
                    td.url = orig_url.clone();
                } else if orig_url != &td.url && !td.fallback_urls.contains(orig_url) {
                    td.fallback_urls.push(orig_url.clone());
                }
            }
        }

        if td.url.is_empty() && td.torrent_target.is_none() {
            return Err(format!(
                "Could not find a matching stream on YouTube, Bandcamp, or SoundCloud for '{}'.",
                existing_song.title
            ));
        }

        (td, existing_song.external_cover_url)
    } else {
        let mut parsed = match platform {
            Platform::Bandcamp => bandcamp::parse_track(url).await?,
            Platform::Youtube => youtube::parse_track(url).await?,
            Platform::Soundcloud => soundcloud::parse_track(url).await?,
            Platform::Local => {
                return Err("Local files cannot be processed via remote URL pipelines.".into());
            }
        };
        if let Some(ref ov_list) = task_overrides.as_ref().and_then(|o| o.tracks.as_ref()) {
            if let Some(ov) = ov_list.first() {
                if let Some(ref ov_cands) = ov.candidates {
                    if !ov_cands.is_empty() {
                        let _ = crate::download::album::apply_candidate_to_track(
                            &mut parsed.0,
                            ov_cands,
                            effective_format.as_deref(),
                            ov.selected_candidate_idx.unwrap_or(0),
                        );
                    }
                }
            }
        }
        parsed
    };

    if !url.starts_with("resolve:") && task_overrides.as_ref().and_then(|item| item.tracks.as_ref())
        .is_none_or(|items| items.is_empty())
    {
        if let Some(registry) = &registry {
            if registry.uses_automatic_quality(task_id).await {
                let artist = track_info.album_artist.clone();
                let album = track_info.album.clone();
                super::discovery::discover_automatic_sources(
                    app_handle, task_id, &artist, &album, std::slice::from_mut(&mut track_info),
                    |_, _| false, effective_format.as_deref().unwrap_or("best"),
                ).await;
            }
        }
    }

    // The library already has the album of this track (e.g. downloaded from another platform): file it there.
    if track_info.track_number.is_none() {
        if let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<crate::db::db::Database>>() {
            let artist = track_info.artists.first().cloned().unwrap_or_else(|| track_info.album_artist.clone());
            let db = db_state.lock().await;
            if let Ok(rows) = db.list_album_sources_for_artist(&artist).await {
                if let Some((album, album_artist, number)) =
                    crate::library_index::sources::find_in_sources(&rows, &artist, &track_info.title)
                {
                    track_info.album = album;
                    track_info.album_artist = album_artist;
                    track_info.track_number = Some(number);
                }
            }
        }
    }

    let album_title = if track_info.album.is_empty() {
        "Singles".to_string()
    } else {
        track_info.album.clone()
    };

    if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_metadata(
                task_id,
                &album_title,
                &track_info.album_artist,
                external_cover_url.as_deref(),
                track_info.release_year,
                vec![(track_info.title.clone(), track_info.track_number, track_info.disc_number)],
            )
            .await;
    }

    let _ = app_handle.emit(
        "download-album-meta",
        AlbumMetaEvent {
            task_id: task_id.to_string(),
            title: album_title,
            artist: track_info.album_artist.clone(),
            cover_url: external_cover_url.clone(),
            release_year: track_info.release_year,
            tracks: vec![TrackMetaItem {
                title: track_info.title.clone(),
                track_number: track_info.track_number.or(Some(1)),
                disc_number: track_info.disc_number,
            }],
        },
    );

    let (temp_file_path, downloaded_track) = if let Some(ref target) = track_info.torrent_target {
        let req = crate::torrent::download::TorrentFileRequest {
            track_index: 0,
            file_index: target.file_index,
            file_size: target.file_size,
            track: track_info.clone(),
        };
        let completed_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cache_root = crate::torrent::download::torrent_cache_root(std::path::Path::new(output_dir));
        let staging = crate::torrent::download::TorrentStagingCleanup(
            crate::torrent::download::torrent_staging_dir(&cache_root, task_id),
        );
        let result = crate::torrent::download::download_torrent_files(
            app_handle,
            task_id,
            std::path::Path::new(output_dir),
            &target.magnet_uri,
            &[req],
            1,
            completed_counter,
            None,
        )
        .await;
        staged_torrent_track(staging, result, &track_info.title)?
    } else {
        let mut wanted = vec![track_info.clone()];
        if super::youtube_sign_in::is_torrent_download(effective_format.as_deref(), &wanted) {
            let skipped = super::youtube_sign_in::leave_out_youtube_unless_signed_in(
                &mut wanted,
                super::youtube_sign_in::has_session(app_handle),
            ).await;
            if skipped > 0 {
                return Err(super::youtube_sign_in::skipped_message(skipped));
            }
        }
        let track = wanted.pop().ok_or("There is no track to download.")?;
        ytdlp::download_single(app_handle, &track, output_dir, task_id).await?
    };

    let mut cover_path = None;
    if let Some(cover_url) = external_cover_url {
        let temp_dir = temp_file_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(output_dir));

        if download_album_cover(&cover_url, &temp_dir).await.is_ok() {
            cover_path = Some(temp_dir.join("cover.jpg"));
        }
    }

    let root_library_dir = PathBuf::from(output_dir);
    Ok((
        root_library_dir,
        vec![(temp_file_path, downloaded_track)],
        cover_path,
    ))
}

/// Keeps the staging folder only when it holds a track to save; otherwise dropping `staging` removes it.
fn staged_torrent_track(
    staging: crate::torrent::download::TorrentStagingCleanup,
    result: Result<crate::torrent::download::TorrentTransferResult, String>,
    title: &str,
) -> Result<(PathBuf, TrackDownload), String> {
    let selected = selected_torrent_track(result, title)?;
    staging.keep();
    Ok(selected)
}

fn selected_torrent_track(
    result: Result<crate::torrent::download::TorrentTransferResult, String>,
    title: &str,
) -> Result<(PathBuf, TrackDownload), String> {
    let mut result = result?;
    if !result.completed.is_empty() {
        let (path, track, _) = result.completed.remove(0);
        return Ok((path, track));
    }
    Err(result.failure.unwrap_or_else(|| format!("Torrent transfer failed for '{title}'.")))
}

#[cfg(test)]
mod torrent_selection_tests {
    use super::*;
    use crate::torrent::download::TorrentTransferResult;

    #[test]
    fn verified_album_lossless_can_upgrade_a_single_track_and_noop_explains_itself() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
        use crate::types::QualityTier;
        let stream = AudioCandidate {
            source: AudioSource::HttpStream { platform: "youtube".into(), url: "https://youtu.be/id".into(), format_id: None, direct_stream_url: None, expires_at: None },
            quality_tier: AudioQualityTier::HighLossy, codec: "Opus".into(), bitrate_kbps: Some(160),
            sample_rate_hz: None, duration_ms: 200_000, match_confidence: 1.0, is_strict_match: true,
            matched_title: None, matched_artist: None, display_label: "Opus".into(),
        };
        assert_eq!(upgrade_candidate_index(&[stream.clone()], Some("best"), 0, QualityTier::High, true).unwrap_err(), NO_TRACK_UPGRADE);
        let torrent = AudioCandidate {
            source: AudioSource::Torrent {
                info_hash: "hash".into(), magnet_uri: "magnet:hash".into(), file_index: 3,
                file_path: "04 Sunshine.flac".into(), file_size: 1000, seeders: 3,
            },
            quality_tier: AudioQualityTier::Lossless, codec: "FLAC".into(), ..stream.clone()
        };
        let candidates = [stream, torrent];
        let index = upgrade_candidate_index(&candidates, Some("best"), 1, QualityTier::High, true).unwrap();
        let mut track = TrackDownload::default();
        crate::download::album::apply_candidate_to_track(&mut track, &candidates, Some("best"), index);
        assert_eq!(track.torrent_target.unwrap().file_index, 3);
    }

    #[test]
    fn failed_selected_torrent_is_not_replaced_by_http() {
        let track = TrackDownload { url: "https://example.com/stream".into(), ..Default::default() };
        let result = TorrentTransferResult {
            completed: Vec::new(), failed_tracks: vec![(0, track)],
            failure: Some("No peers responded".into()),
        };
        assert_eq!(selected_torrent_track(Ok(result), "Song").unwrap_err(), "No peers responded");
        assert_eq!(selected_torrent_track(Err("Task cancelled".into()), "Song").unwrap_err(), "Task cancelled");
    }

    #[test]
    fn selected_torrent_returns_its_completed_file() {
        let track = TrackDownload { title: "Song".into(), ..Default::default() };
        let result = TorrentTransferResult {
            completed: vec![(PathBuf::from("song.flac"), track, 0)], failed_tracks: Vec::new(), failure: None,
        };
        assert_eq!(selected_torrent_track(Ok(result), "Song").unwrap().0, PathBuf::from("song.flac"));
    }

    fn staged_folder_with_track(root: &std::path::Path) -> PathBuf {
        let dir = root.join("staging");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("track.flac"), b"audio").unwrap();
        dir
    }

    async fn wait_until_removed(path: &std::path::Path) -> bool {
        for _ in 0..100 {
            if !path.exists() { return true; }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        !path.exists()
    }

    #[tokio::test]
    async fn failed_torrent_transfer_removes_its_staging_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = staged_folder_with_track(root.path());
        let staging = crate::torrent::download::TorrentStagingCleanup(dir.clone());
        let result = staged_torrent_track(staging, Err("No peers responded".into()), "Song");
        assert_eq!(result.err(), Some("No peers responded".to_string()));
        assert!(wait_until_removed(&dir).await);
    }

    #[tokio::test]
    async fn transfer_without_a_completed_file_removes_its_staging_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = staged_folder_with_track(root.path());
        let staging = crate::torrent::download::TorrentStagingCleanup(dir.clone());
        let result = TorrentTransferResult { completed: Vec::new(), failed_tracks: Vec::new(), failure: None };
        assert!(staged_torrent_track(staging, Ok(result), "Song").is_err());
        assert!(wait_until_removed(&dir).await);
    }

    #[tokio::test]
    async fn completed_torrent_transfer_keeps_its_staging_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = staged_folder_with_track(root.path());
        let staging = crate::torrent::download::TorrentStagingCleanup(dir.clone());
        let track = TrackDownload { title: "Song".into(), ..Default::default() };
        let result = TorrentTransferResult {
            completed: vec![(dir.join("track.flac"), track, 0)], failed_tracks: Vec::new(), failure: None,
        };
        let (path, _) = staged_torrent_track(staging, Ok(result), "Song").unwrap();
        assert_eq!(path, dir.join("track.flac"));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(dir.join("track.flac").exists());
    }
}
