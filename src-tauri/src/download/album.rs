use super::youtube;
use super::{bandcamp, ytdlp};
use crate::download::types::DownloadResult;
use crate::types::Platform;
use crate::utils::{remove_empty_parents_up_to, sanitize_fs};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

pub async fn process_album_url(
    platform: Platform,
    url: &str,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<DownloadResult, String> {
    let root_library_dir = PathBuf::from(output_dir);

    let album_info = match platform {
        Platform::Bandcamp => bandcamp::parse_album(url).await?,
        Platform::Youtube => youtube::parse_album(url).await?,
        Platform::Local => {
            return Err("Local files cannot be processed via remote URL pipelines.".into());
        }
    };

    let mut tracks = album_info.tracks;
    for track in &mut tracks {
        track.album_artist = album_info.album_artist.clone();
    }

    let target_library_dir = root_library_dir
        .join(sanitize_fs(&album_info.album_artist))
        .join(sanitize_fs(&album_info.title));

    let folder_created = target_library_dir != root_library_dir;
    if folder_created {
        tokio::fs::create_dir_all(&target_library_dir)
            .await
            .map_err(|e| format!("Failed creating album path: {e}"))?;
    }

    let mut cover_path = None;
    if let Some(cover_url) = &album_info.external_cover_url {
        if download_album_cover(cover_url, &target_library_dir)
            .await
            .is_ok()
        {
            cover_path = Some(target_library_dir.join("cover.jpg"));
        }
    }

    match ytdlp::download_batch(app_handle, &tracks, output_dir, task_id).await {
        Ok(files) => Ok((target_library_dir, files, cover_path)),
        Err(err) => {
            if folder_created
                && target_library_dir.exists()
                && fs::remove_dir_all(&target_library_dir).is_ok()
            {
                remove_empty_parents_up_to(&target_library_dir, &root_library_dir);
            }
            Err(err)
        }
    }
}

pub async fn download_album_cover(url: &str, output_dir: &Path) -> Result<(), String> {
    let bytes = reqwest::get(url)
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    tokio::fs::write(output_dir.join("cover.jpg"), &bytes)
        .await
        .map_err(|e| e.to_string())
}
