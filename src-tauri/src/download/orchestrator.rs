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
    let is_album = task.payload.is_album();

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
            process_album_url(task.platform, &url, &task.output_dir, &app_handle, &task_id).await
        }
        DownloadPayload::TrackURL(url) => {
            process_track_url(task.platform, &url, &task.output_dir, &app_handle, &task_id).await
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

    for (i, (temp_file, track)) in downloaded_files.iter().enumerate() {
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
                let _ = fs::remove_dir_all(stage_dir);
            }
        }
    }
}
