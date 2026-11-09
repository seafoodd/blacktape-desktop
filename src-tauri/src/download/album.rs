use super::youtube;
use super::{bandcamp, soundcloud, ytdlp};
use crate::db::db::Database;
use crate::download::soundcloud::SkippedTrack;
use crate::download::types::{
    AlbumDownload, AlbumMetaEvent, DownloadResult, ProgressEvent, TrackDownload, TrackMetaItem,
};
use crate::library_index::sources::build_album_source;
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

/// Stores the whole track list of the album in `album_sources` (library-sync D5) so the library can tell
/// partial from complete albums and list the missing songs. Returns whether a row was written. The key and the
/// stored `album` / `album_artist` are what `post_processor` writes into `songs.album` / `songs.album_artist`.
pub async fn write_album_source(
    db: &Database,
    platform: Platform,
    url: &str,
    album: &AlbumDownload,
    skipped: &[SkippedTrack],
    now: i64,
) -> Result<bool, sqlx::Error> {
    match build_album_source(platform, url, album, skipped, now) {
        Some(source) => {
            db.upsert_album_source(&source).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Best effort: a failure here is only logged, the download must not depend on the bookkeeping.
async fn record_album_source(
    app_handle: &AppHandle,
    platform: Platform,
    url: &str,
    album: &AlbumDownload,
    skipped: &[SkippedTrack],
) {
    let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<Database>>() else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let db = db_state.lock().await;
    if let Err(e) = write_album_source(&db, platform, url, album, skipped, now).await {
        eprintln!("[album_sources] could not store the source of {url}: {e}");
    }
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
    let mut skipped_tracks: Vec<SkippedTrack> = Vec::new();
    let mut album_info = match platform {
        Platform::Bandcamp => bandcamp::parse_album(url).await?,
        Platform::Youtube => youtube::parse_album(url).await?,
        Platform::Soundcloud => {
            let (album, skipped) = soundcloud::parse_album_with_skipped(url).await?;
            if !skipped.is_empty() {
                skipped_note = Some(soundcloud::describe_skipped(&skipped));
            }
            skipped_tracks = skipped;
            album
        }
        Platform::Local => {
            return Err("Local files cannot be processed via remote URL pipelines.".into());
        }
    };

    let album_artist = album_info.album_artist.clone();
    for track in &mut album_info.tracks {
        track.album_artist = album_artist.clone();
    }
    // Before the selection: the source holds the whole album, also when only some tracks are downloaded.
    record_album_source(app_handle, platform, url, &album_info, &skipped_tracks).await;

    let mut tracks = album_info.tracks;
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
    use crate::db::db::tests::setup_test_db;
    use crate::library_index::normalize::normalize_url_key;

    fn album_of(urls: &[&str]) -> AlbumDownload {
        AlbumDownload {
            title: "The Album".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            tracks: urls.iter().enumerate().map(|(i, u)| track(u, i as i32 + 1)).collect(),
            genres: None,
            release_year: Some(2020),
            external_cover_url: Some("https://img/c.jpg".to_string()),
        }
    }

    async fn db_with_schema() -> Database {
        let db = setup_test_db().await;
        db.ensure_album_sources_schema().await.unwrap();
        db
    }

    #[tokio::test]
    async fn write_album_source_stores_the_whole_album() {
        let db = db_with_schema().await;
        let url = "https://a.bandcamp.com/album/the-album";
        let album = album_of(&["https://a.bandcamp.com/track/1", "https://a.bandcamp.com/track/2"]);
        assert!(write_album_source(&db, Platform::Bandcamp, url, &album, &[], 42).await.unwrap());

        let row = db
            .get_album_source_by_key(&normalize_url_key(url).unwrap())
            .await
            .unwrap()
            .expect("row");
        assert_eq!(row.platform, "bandcamp");
        assert_eq!(row.album, "The Album");
        assert_eq!(row.album_artist, "Artist");
        assert_eq!(row.fetched_at, 42);
        assert_eq!(
            row.track_urls,
            r#"["https://a.bandcamp.com/track/1","https://a.bandcamp.com/track/2"]"#
        );
    }

    #[tokio::test]
    async fn write_album_source_updates_the_row_on_a_second_download() {
        let db = db_with_schema().await;
        let url = "https://a.bandcamp.com/album/the-album";
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1"]), &[], 1).await.unwrap();
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1", "https://u/2"]), &[], 2)
            .await
            .unwrap();
        let rows = db.list_album_sources_for_artist("Artist").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fetched_at, 2);
        assert_eq!(rows[0].track_urls, r#"["https://u/1","https://u/2"]"#);
    }

    #[tokio::test]
    async fn write_album_source_skips_albums_without_a_usable_key_or_tracks() {
        let db = db_with_schema().await;
        assert!(!write_album_source(&db, Platform::Bandcamp, "", &album_of(&["https://u/1"]), &[], 1).await.unwrap());
        assert!(!write_album_source(&db, Platform::Bandcamp, "https://a.bandcamp.com/album/x", &album_of(&[]), &[], 1)
            .await
            .unwrap());
        assert!(db.list_album_sources_for_artist("Artist").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn write_album_source_reports_a_missing_table_instead_of_panicking() {
        let db = setup_test_db().await; // no album_sources table
        let result = write_album_source(
            &db,
            Platform::Bandcamp,
            "https://a.bandcamp.com/album/x",
            &album_of(&["https://u/1"]),
            &[],
            1,
        )
        .await;
        assert!(result.is_err());
    }

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
