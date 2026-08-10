use super::{bandcamp, youtube};
use crate::download::album::download_album_cover;
use crate::download::types::{DownloadResult, ProgressEvent};
use crate::download::ytdlp;
use crate::types::Platform;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

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

    let (track_info, external_cover_url) = match platform {
        Platform::Bandcamp => bandcamp::parse_track(url).await?,
        Platform::Youtube => youtube::parse_track(url).await?,
        Platform::Local => {
            return Err("Local files cannot be processed via remote URL pipelines.".into());
        }
    };

    let (temp_file_path, downloaded_track) =
        ytdlp::download_single(app_handle, &track_info, output_dir, task_id).await?;

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
