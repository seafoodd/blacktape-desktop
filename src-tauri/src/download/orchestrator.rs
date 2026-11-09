use crate::db::db::Database;
use crate::download::album::process_album_url;
use crate::download::post_processor::process_and_move_track;
use crate::download::track::process_track_url;
use crate::download::types::{DownloadPayload, DownloadTask, ProgressEvent, TaskEvent};
use crate::download::ytdlp;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

pub async fn handle_download_task(task: DownloadTask, app_handle: AppHandle) -> Result<(), String> {
    let task_id = task.id.clone();
    let mut is_album = task.payload.is_album();

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
                            eprintln!("[download] album route of {url} failed ({e}); downloading it as a single");
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
        DownloadPayload::Single(track) => {
            ytdlp::download_batch(&app_handle, &[track], &task.output_dir, &task_id)
                .await
                .map(|files| (output_path, files, None))
        }
        DownloadPayload::Batch { tracks } => {
            ytdlp::download_batch(&app_handle, &tracks, &task.output_dir, &task_id)
                .await
                .map(|files| (output_path, files, None))
        }
    };

    let (target_dir, downloaded_files, cover_path) = match download_result {
        Ok(res) => res,
        Err(e) => {
            emit_event(
                &app_handle,
                "download-task-failed",
                &task_id,
                &format!("Download failed: {e}"),
            );
            return Err(e);
        }
    };

    let mut new_songs = Vec::new();
    let total = downloaded_files.len();

    // A single track whose album the library already has (other platform): number and album come from it.
    let mut placed: Vec<Option<(String, String, i32)>> = vec![None; downloaded_files.len()];
    if !is_album {
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

        let _ = fs::create_dir_all(&track_target_dir);

        let target_cover_path = if let Some(ref src_cover) = cover_path {
            let dest_cover = track_target_dir.join("cover.jpg");
            if src_cover.exists() && !dest_cover.exists() {
                let _ = fs::copy(src_cover, &dest_cover);
            }
            Some(dest_cover)
        } else {
            None
        };

        match process_and_move_track(
            temp_file,
            track,
            &track_target_dir,
            target_cover_path.as_deref(),
            &covers_dir,
            task.platform,
        ) {
            Ok(song) => new_songs.push(song),
            Err(err) => eprintln!(
                "[Post-Processor Error] Mapping failed for {}: {err}",
                track.title
            ),
        }
    }

    cleanup_staging_dir(&downloaded_files);

    if !new_songs.is_empty() {
        let db_state = app_handle.state::<tokio::sync::Mutex<Database>>();
        let db = db_state.lock().await;
        println!("Inserting songs to db: {new_songs:?}");
        if let Err(e) = db.insert_songs(new_songs).await {
            eprintln!("[Queue Worker] DB Insertion error: {e}");
        }
    }

    emit_event(
        &app_handle,
        "download-task-completed",
        &task_id,
        "Pipeline complete",
    );
    Ok(())
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
