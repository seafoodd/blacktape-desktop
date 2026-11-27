use crate::db::db::Database;
use crate::download::album::process_album_url;
use crate::download::post_processor::process_and_move_track;
use crate::download::track::process_track_url;
use crate::download::types::{DownloadPayload, DownloadTask, ProgressEvent, TaskEvent, TrackDownload};
use crate::download::ytdlp;
use crate::types::Platform;
use crate::Song;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, UserAttentionType};

/// What the queue shows while a download waits for yt-dlp, deno or ffmpeg to be installed the first time.
const WAITING_FOR_SETUP: &str = "Waiting for downloader setup…";

pub async fn save_downloaded_track<R: tauri::Runtime>(
    temp_file: &Path,
    track: &TrackDownload,
    track_target_dir: &Path,
    cover_path: Option<&Path>,
    covers_dir: &Path,
    platform: Platform,
    app_handle: &AppHandle<R>,
    task_id: &str,
    track_index: usize,
) -> Result<Song, String> {
    // Let a save already moving files and updating the DB finish before cancellation.
    let _saving = match app_handle.try_state::<crate::download::TaskRegistry>() {
        Some(registry) => registry.begin_save(task_id).await?,
        None => None,
    };
    let replace_song_id = app_handle
        .try_state::<crate::download::TaskRegistry>()
        .map(|reg| async move { reg.replace_song_id_for(task_id, track_index).await });
    let replace_song_id = match replace_song_id {
        Some(fut) => fut.await,
        None => None,
    };

    if let Some(old_id) = replace_song_id {
        return replace_upgraded_track(
            old_id,
            temp_file,
            track,
            track_target_dir,
            cover_path,
            covers_dir,
            platform,
            app_handle,
            task_id,
            track_index,
        )
        .await;
    }

    let _ = fs::create_dir_all(track_target_dir);

    let target_cover_path = if let Some(src_cover) = cover_path {
        let dest_cover = track_target_dir.join("cover.jpg");
        if src_cover.exists() && !dest_cover.exists() {
            let _ = fs::copy(src_cover, &dest_cover);
        }
        Some(dest_cover)
    } else {
        None
    };

    let song = process_and_move_track(
        temp_file,
        track,
        track_target_dir,
        target_cover_path.as_deref(),
        covers_dir,
        platform,
    )?;

    if let Some(ref cover_url) = song.cover_url {
        for dir in crate::asset_scope::cover_dirs(std::iter::once(cover_url.as_str())) {
            crate::asset_scope::allow_dir(app_handle, &dir, false);
        }
    }

    if let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<Database>>() {
        let db = db_state.lock().await;
        tracing::debug!(song = %song.title, "Inserting song to DB");
        if let Err(e) = db.insert_songs(vec![song.clone()]).await {
            tracing::error!(error = %e, "DB insertion error");
        }
    }

    let _ = app_handle.emit(
        "download-track-saved",
        serde_json::json!({
            "task_id": task_id,
            "track_index": track_index,
            "song": song,
        }),
    );

    Ok(song)
}

async fn replace_upgraded_track<R: tauri::Runtime>(
    old_id: i64,
    temp_file: &Path,
    track: &TrackDownload,
    track_target_dir: &Path,
    cover_path: Option<&Path>,
    covers_dir: &Path,
    platform: Platform,
    app_handle: &AppHandle<R>,
    task_id: &str,
    track_index: usize,
) -> Result<Song, String> {
    use lofty::file::AudioFile;
    use lofty::probe::Probe;

    // 1. Verify downloaded staging audio file BEFORE touching old file or DB.
    if !temp_file.exists() {
        return Err("Upgraded audio file is missing on disk.".to_string());
    }
    let meta = fs::metadata(temp_file).map_err(|e| format!("Failed reading upgraded file: {e}"))?;
    if meta.len() == 0 {
        return Err("Upgraded audio file is empty.".to_string());
    }
    let probe = Probe::open(temp_file)
        .map_err(|e| format!("Upgraded file failed probe: {e}"))?
        .guess_file_type()
        .map_err(|e| format!("Upgraded file unrecognized audio format: {e}"))?
        .read()
        .map_err(|e| format!("Upgraded file failed decode check: {e}"))?;
    if probe.properties().duration().as_millis() == 0 {
        return Err("Upgraded audio file has zero duration.".to_string());
    }

    let db_state = app_handle
        .try_state::<tokio::sync::Mutex<Database>>()
        .ok_or_else(|| "Database state unavailable for song upgrade".to_string())?;
    let old_song = {
        let db = db_state.lock().await;
        db.require_song_by_id(old_id).await?
    };

    // Keep the upgraded file in the same album directory as the original library track when it exists.
    let effective_dir = Path::new(&old_song.path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| track_target_dir.to_path_buf());
    let _ = fs::create_dir_all(&effective_dir);

    let target_cover_path = if let Some(src_cover) = cover_path {
        let dest_cover = effective_dir.join("cover.jpg");
        if src_cover.exists() && !dest_cover.exists() {
            let _ = fs::copy(src_cover, &dest_cover);
        }
        Some(dest_cover)
    } else {
        let existing_cover = effective_dir.join("cover.jpg");
        if existing_cover.exists() {
            Some(existing_cover)
        } else {
            None
        }
    };

    let trashed = crate::trash::move_old_file_for_upgrade(app_handle, &db_state, &old_song).await?;

    let mut song = match process_and_move_track(
        temp_file,
        track,
        &effective_dir,
        target_cover_path.as_deref(),
        covers_dir,
        platform,
    ) {
        Ok(s) => s,
        Err(e) => {
            if let Some((_, ref trash_dst, ref orig_path, _, _)) = trashed {
                crate::trash::rollback_moved_file(trash_dst, Path::new(orig_path)).await;
            }
            return Err(e);
        }
    };

    song.id = Some(old_id);
    if song.lyrics.is_none() {
        song.lyrics = old_song.lyrics.clone();
        song.lyrics_source = old_song.lyrics_source.clone();
    }
    if song.cover_url.is_none() {
        song.cover_url = old_song.cover_url.clone();
    }
    if song.external_cover_url.is_none() {
        song.external_cover_url = old_song.external_cover_url.clone();
    }

    if let Some(ref cover_url) = song.cover_url {
        for dir in crate::asset_scope::cover_dirs(std::iter::once(cover_url.as_str())) {
            crate::asset_scope::allow_dir(app_handle, &dir, false);
        }
    }

    let trash_strs = trashed.as_ref().map(|(json, dst, orig, sz, ts)| {
        (
            json.as_str(),
            orig.as_str(),
            dst.to_string_lossy().into_owned(),
            *sz,
            *ts,
        )
    });

    {
        let db = db_state.lock().await;
        let trash_ref = trash_strs
            .as_ref()
            .map(|(json, orig, dst, sz, ts)| (*json, *orig, dst.as_str(), *sz, *ts));
        if let Err(e) = db
            .replace_song_file_and_record_trash(old_id, &song, trash_ref)
            .await
        {
            drop(db);
            let _ = fs::remove_file(&song.path);
            if let Some((_, ref trash_dst, ref orig_path, _, _)) = trashed {
                crate::trash::rollback_moved_file(trash_dst, Path::new(orig_path)).await;
            }
            return Err(format!("Failed updating library database row for upgrade: {e}"));
        }
    }

    if trashed.is_some() {
        let _ = app_handle.emit(crate::trash::TRASH_CHANGED_EVENT, ());
    }
    let _ = app_handle.emit(
        "download-track-saved",
        serde_json::json!({
            "task_id": task_id,
            "track_index": track_index,
            "song": song,
        }),
    );

    Ok(song)
}

pub async fn handle_download_task(task: DownloadTask, app_handle: AppHandle) -> Result<(), String> {
    let task_id = task.id.clone();
    let mut is_album = task.payload.is_album();

    {
        use crate::download::tools::{self, Tool};
        if tools::setting_up(&tools::snapshot(), &Tool::ALL) {
            emit_event(&app_handle, "download-task-started", &task_id, WAITING_FOR_SETUP);
            // a setup that failed is reported once the download starts
            let _ = tools::wait_for(&Tool::ALL, tools::SETUP_WAIT).await;
        }
    }
    emit_event(
        &app_handle,
        "download-task-started",
        &task_id,
        "Download task started",
    );

    let app_data = app_handle.path().app_data_dir().unwrap();
    let covers_dir = app_data.join("covers");
    let _ = fs::create_dir_all(&covers_dir);

    let output_path = PathBuf::from(&task.output_dir);
    let placement = match &task.payload {
        DownloadPayload::PlacedTrack { placement, .. } => Some(placement.clone()),
        _ => None,
    };

    let (target_url, track_info) = match &task.payload {
        DownloadPayload::Single(t) => (Some(t.url.clone()), Some(format!("{} - {}", t.artists.join(", "), t.title))),
        DownloadPayload::Batch { tracks } => (tracks.first().map(|t| t.url.clone()), tracks.first().map(|t| format!("{} - {}", t.artists.join(", "), t.title))),
        DownloadPayload::AlbumURL(u) => (Some(u.clone()), None),
        DownloadPayload::AlbumSelection { url, .. } => (Some(url.clone()), None),
        DownloadPayload::TrackURL(u) => (Some(u.clone()), None),
        DownloadPayload::PlacedTrack { url, placement } => (Some(url.clone()), Some(format!("{} - {}", placement.album_artist, placement.title))),
    };

    let download_result = match task.payload {
        DownloadPayload::AlbumURL(url) => {
            process_album_url(task.platform, &url, None, &task.output_dir, &app_handle, &task_id)
                .await
        }
        DownloadPayload::AlbumSelection { url, track_urls } => {
            process_album_url(
                task.platform,
                &url,
                Some(&track_urls),
                &task.output_dir,
                &app_handle,
                &task_id,
            )
            .await
        }
        DownloadPayload::TrackURL(url) => {
            // A Bandcamp track of an album goes through the album pipeline: album folder, cover, track number.
            let album_url = if task.platform == crate::types::Platform::Bandcamp {
                crate::download::bandcamp::find_album_url(&url).await
            } else {
                None
            };
            let via_album = match album_url {
                Some(album_url) => {
                    match process_album_url(
                        task.platform,
                        &album_url,
                        Some(std::slice::from_ref(&url)),
                        &task.output_dir,
                        &app_handle,
                        &task_id,
                    )
                    .await
                    {
                        Ok(res) => Some(res),
                        Err(e) => {
                            tracing::warn!(url = %url, error = %e, "Album route failed; downloading as a single");
                            None
                        }
                    }
                }
                None => None,
            };
            match via_album {
                Some(res) => {
                    is_album = true;
                    Ok(res)
                }
                None => {
                    process_track_url(task.platform, &url, &task.output_dir, &app_handle, &task_id).await
                }
            }
        }
        DownloadPayload::PlacedTrack { url, .. } => {
            process_track_url(task.platform, &url, &task.output_dir, &app_handle, &task_id).await
        }
        DownloadPayload::Single(track) => {
            ytdlp::download_batch(&app_handle, &[track], &task.output_dir, &task_id)
                .await
                .map(|files| (output_path, files, None))
        }
        DownloadPayload::Batch { tracks } => {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<(PathBuf, TrackDownload, usize)>(32);
            let app_handle_clone = app_handle.clone();
            let task_id_str = task_id.clone();
            let covers_dir_clone = covers_dir.clone();
            let output_path_clone = output_path.clone();

            let consumer = tokio::spawn(async move {
                let mut songs = Vec::new();
                while let Some((temp_file, track, index)) = rx.recv().await {
                    let artist_dir = if track.album_artist.is_empty() {
                        "Unknown Artist"
                    } else {
                        &track.album_artist
                    };
                    let album_dir = if track.album.is_empty() {
                        "Singles"
                    } else {
                        &track.album
                    };
                    let track_target_dir = output_path_clone
                        .join(crate::utils::sanitize_fs(artist_dir))
                        .join(crate::utils::sanitize_fs(album_dir));
                    match save_downloaded_track(
                        &temp_file,
                        &track,
                        &track_target_dir,
                        None,
                        &covers_dir_clone,
                        task.platform,
                        &app_handle_clone,
                        &task_id_str,
                        index,
                    )
                    .await
                    {
                        Ok(song) => songs.push(song),
                        Err(err) => tracing::error!(
                            title = %track.title,
                            error = %err,
                            "Failed to save downloaded track"
                        ),
                    }
                }
                songs
            });

            let download_res = ytdlp::download_batch_streaming(
                &app_handle,
                &tracks,
                &task.output_dir,
                &task_id,
                Some(tx),
            )
            .await;

            let _ = consumer.await;
            download_res.map(|files| {
                cleanup_staging_dir(&files);
                (output_path, Vec::new(), None)
            })
        }
    };

    let (target_dir, downloaded_files, cover_path) = match download_result {
        Ok(res) => res,
        Err(e) => {
            // yt-dlp failures are reported per track with full stderr/stdout in ytdlp.rs.
            // If the failure occurred outside yt-dlp (e.g. metadata parsing, album discovery, cover fetching),
            // report it here with the task's url and track context.
            if e != super::youtube_sign_in::CANCELLED && e != super::track::NO_TRACK_UPGRADE
                && !super::youtube_sign_in::is_skipped_message(&e)
                && !e.starts_with("ERROR:") && !e.starts_with("[yt-dlp") && !e.contains("No such file or directory") {
                crate::telemetry::report_download_error_with_context(
                    &format!("{:?}", task.platform).to_lowercase(),
                    "download",
                    &e,
                    target_url.as_deref(),
                    track_info.as_deref(),
                    None,
                );
            }
            emit_event(
                &app_handle,
                "download-task-failed",
                &task_id,
                &format!("Download failed: {e}"),
            );
            flash_taskbar(&app_handle);
            return Err(e);
        }
    };

    let total = downloaded_files.len();

    // A single track whose album the library already has (other platform): number and album come from it.
    let mut placed: Vec<Option<(String, String, i32)>> = vec![None; downloaded_files.len()];
    if !is_album && placement.is_none() {
        if let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<Database>>() {
            let db = db_state.lock().await;
            for (i, (_, track)) in downloaded_files.iter().enumerate() {
                if track.track_number.is_some() {
                    continue;
                }
                let artist = track.artists.first().cloned().unwrap_or_else(|| track.album_artist.clone());
                if let Ok(rows) = db.list_album_sources_for_artist(&artist).await {
                    placed[i] = crate::library_index::sources::find_in_sources(&rows, &artist, &track.title);
                }
            }
        }
    }

    for (i, (temp_file, original)) in downloaded_files.iter().enumerate() {
        let mut owned_track = original.clone();
        if let Some((album, album_artist, number)) = placed[i].take() {
            owned_track.album = album;
            owned_track.album_artist = album_artist;
            owned_track.track_number = Some(number);
        }
        if let Some(placement) = &placement {
            placement.apply(&mut owned_track);
        }
        let track = &owned_track;
        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.clone(),
                current: i + 1,
                total,
                track_title: track.title.clone(),
            },
        );

        let track_target_dir = if is_album {
            target_dir.clone()
        } else {
            let artist_dir = if track.album_artist.is_empty() {
                "Unknown Artist"
            } else {
                &track.album_artist
            };
            let album_dir = if track.album.is_empty() {
                "Singles"
            } else {
                &track.album
            };

            target_dir
                .join(crate::utils::sanitize_fs(artist_dir))
                .join(crate::utils::sanitize_fs(album_dir))
        };

        let actual_platform =
            crate::download::album::detect_platform_from_url(&track.url, task.platform);
        match save_downloaded_track(
            temp_file,
            track,
            &track_target_dir,
            cover_path.as_deref(),
            &covers_dir,
            actual_platform,
            &app_handle,
            &task_id,
            i,
        )
        .await
        {
            Ok(_) => {}
            Err(err) => tracing::error!(
                title = %track.title,
                error = %err,
                "Post-processor track mapping failed"
            ),
        }
    }

    cleanup_staging_dir(&downloaded_files);

    emit_event(
        &app_handle,
        "download-task-completed",
        &task_id,
        "Pipeline complete",
    );
    Ok(())
}

/// A failed download needs the user's decision, so a window that is in the background (or minimized) asks for
/// attention: the taskbar button flashes until the window is focused. A focused window needs nothing.
fn needs_attention(focused: bool) -> bool {
    !focused
}

fn flash_taskbar(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if needs_attention(window.is_focused().unwrap_or(false)) {
            let _ = window.request_user_attention(Some(UserAttentionType::Informational));
        }
    }
}

fn emit_event(app: &AppHandle, event: &str, task_id: &str, msg: &str) {
    let _ = app.emit(
        event,
        TaskEvent {
            task_id: task_id.into(),
            message: msg.into(),
        },
    );
}

fn cleanup_staging_dir(downloaded_files: &[(PathBuf, crate::download::types::TrackDownload)]) {
    if let Some((first_file, _)) = downloaded_files.first() {
        if let Some(stage_dir) = first_file.parent() {
            if stage_dir.exists() {
                let _ = crate::utils::force_remove_dir_all(stage_dir);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unfocused_window_asks_for_attention() {
        assert!(needs_attention(false));
        assert!(!needs_attention(true));
    }

    #[test]
    fn cover_preparation_copies_when_dest_missing() {
        let temp = tempfile::tempdir().unwrap();
        let target_dir = temp.path().join("Album");
        fs::create_dir_all(&target_dir).unwrap();
        let src_cover = temp.path().join("src_cover.jpg");
        fs::write(&src_cover, b"jpeg_data").unwrap();

        let dest_cover = target_dir.join("cover.jpg");
        assert!(!dest_cover.exists());

        if src_cover.exists() && !dest_cover.exists() {
            let _ = fs::copy(&src_cover, &dest_cover);
        }

        assert!(dest_cover.exists());
        assert_eq!(fs::read(&dest_cover).unwrap(), b"jpeg_data");
    }
}
