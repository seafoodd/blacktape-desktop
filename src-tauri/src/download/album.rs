use super::youtube;
use super::{bandcamp, soundcloud, ytdlp};
use crate::download::types::{
    AlbumMetaEvent, DownloadResult, ProgressEvent, TrackDownload, TrackMetaItem,
};
use crate::types::Platform;
use crate::utils::{remove_empty_parents_up_to, sanitize_fs};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// Keeps only the tracks whose url is in `selected`, in album order. Pure.
pub fn select_tracks(tracks: Vec<TrackDownload>, selected: &[String]) -> Vec<TrackDownload> {
    tracks
        .into_iter()
        .filter(|t| selected.iter().any(|url| *url == t.url))
        .collect()
}

/// Downloads the album at `url`; with `selected` only the tracks whose page url is listed (the album
/// folder, cover and track numbers stay those of the whole album).
pub async fn process_album_url(
    platform: Platform,
    url: &str,
    selected: Option<&[String]>,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<DownloadResult, String> {
    let root_library_dir = PathBuf::from(output_dir);

    let mut skipped_note = None;
    let album_info = match platform {
        Platform::Bandcamp => bandcamp::parse_album(url).await?,
        Platform::Youtube => youtube::parse_album(url).await?,
        Platform::Soundcloud => {
            let (album, skipped) = soundcloud::parse_album_with_skipped(url).await?;
            if !skipped.is_empty() {
                skipped_note = Some(soundcloud::describe_skipped(&skipped));
            }
            album
        }
        Platform::Local => {
            return Err("Local files cannot be processed via remote URL pipelines.".into());
        }
    };

    let mut tracks = album_info.tracks;
    for track in &mut tracks {
        track.album_artist = album_info.album_artist.clone();
    }
    if let Some(selected) = selected {
        tracks = select_tracks(tracks, selected);
        if tracks.is_empty() {
            return Err("None of the selected tracks was found in the album.".into());
        }
    }

    let _ = app_handle.emit(
        "download-album-meta",
        AlbumMetaEvent {
            task_id: task_id.to_string(),
            title: album_info.title.clone(),
            artist: album_info.album_artist.clone(),
            cover_url: album_info.external_cover_url.clone(),
            tracks: tracks
                .iter()
                .enumerate()
                .map(|(idx, t)| TrackMetaItem {
                    title: t.title.clone(),
                    track_number: t.track_number.or(Some((idx + 1) as i32)),
                })
                .collect(),
        },
    );

    let target_library_dir = root_library_dir
        .join(sanitize_fs(&album_info.album_artist))
        .join(sanitize_fs(&album_info.title));

    if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_metadata(
                task_id,
                &album_info.title,
                &album_info.album_artist,
                album_info.external_cover_url.as_deref(),
                tracks
                    .iter()
                    .map(|t| (t.title.clone(), t.track_number))
                    .collect(),
            )
            .await;
        registry
            .set_target_dir(
                task_id,
                target_library_dir.clone(),
                root_library_dir.clone(),
            )
            .await;
        if let Some(note) = &skipped_note {
            registry
                .update_task_progress(task_id, 0, tracks.len(), note)
                .await;
        }
    }

    if let Some(note) = skipped_note {
        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.to_string(),
                current: 0,
                total: tracks.len(),
                track_title: note,
            },
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(url: &str, number: i32) -> TrackDownload {
        TrackDownload {
            url: url.to_string(),
            title: format!("t{number}"),
            track_number: Some(number),
            ..TrackDownload::default()
        }
    }

    #[test]
    fn select_tracks_keeps_album_order_and_numbers() {
        let tracks = vec![track("a", 1), track("b", 2), track("c", 3)];
        let picked = select_tracks(tracks, &["c".to_string(), "a".to_string()]);
        let urls: Vec<&str> = picked.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "c"], "album order, not selection order");
        assert_eq!(picked[1].track_number, Some(3), "numbers of the whole album are kept");
    }

    #[test]
    fn select_tracks_ignores_unknown_urls_and_can_be_empty() {
        let picked = select_tracks(vec![track("a", 1)], &["zzz".to_string()]);
        assert!(picked.is_empty());
        assert!(select_tracks(vec![track("a", 1)], &[]).is_empty());
    }
}
