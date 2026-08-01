use crate::db::db::Database;
use crate::types::Platform;
use crate::utils::{
    determine_quality, make_canonical_slug, remove_empty_parents_up_to, sanitize_fs,
};
use crate::Song;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, Tag, TagExt};
use serde::Serialize;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

pub mod bandcamp;
pub mod opus_source;
pub mod youtube;
pub mod ytdlp;

// --- Event Payloads ---
#[derive(Debug, Clone, Serialize)]
pub struct TaskEvent {
    pub task_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub task_id: String,
    pub current: usize,
    pub total: usize,
    pub track_title: String,
}

// --- Core Structs ---
#[derive(Debug, Clone)]
pub struct TrackDownload {
    pub url: String,
    pub file_name: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album_artist: String,
    pub album: String,
    pub track_number: Option<i32>,
    pub genres: Option<Vec<String>>,
    pub release_year: Option<i32>,
    pub source_item_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AlbumDownload {
    pub title: String,
    pub artists: Vec<String>,
    pub album_artist: String,
    pub tracks: Vec<TrackDownload>,
    pub genres: Option<Vec<String>>,
    pub release_year: Option<i32>,
    pub external_cover_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DownloadPayload {
    Single(TrackDownload),
    Batch { tracks: Vec<TrackDownload> },
    AlbumURL(String),
    TrackURL(String),
}

impl DownloadPayload {
    pub fn is_album(&self) -> bool {
        match self {
            DownloadPayload::AlbumURL(_) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub platform: Platform,
    pub payload: DownloadPayload,
    pub output_dir: String,
}

pub struct DownloadQueue {
    pub tx: mpsc::UnboundedSender<DownloadTask>,
}

pub fn init_queue_worker(app: AppHandle) -> mpsc::UnboundedSender<DownloadTask> {
    let (tx, mut rx) = mpsc::unbounded_channel::<DownloadTask>();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        println!("[Queue Worker] Core pipeline active. Awaiting jobs...");

        while let Some(task) = rx.recv().await {
            match handle_download_task(task, app_handle.clone()).await {
                Ok(_) => println!("[Queue Worker] Successfully processed download task."),
                Err(e) => eprintln!("[Queue Worker] Task failure error branch: {}", e),
            }
        }
    });

    tx
}

/// Orchestrates the entire download and post-processing pipeline for a given task.
async fn handle_download_task(task: DownloadTask, app_handle: AppHandle) -> Result<(), String> {
    let task_id = task.id.clone();
    println!("RECEIVED TASK {task:?}");

    let platform = task.platform.clone();
    let output_dir_str = task.output_dir.clone();
    let is_album_payload = task.payload.is_album();

    let _ = app_handle.emit(
        "download-task-started",
        TaskEvent {
            task_id: task_id.clone(),
            message: "Download task started".into(),
        },
    );

    let output_dir_path = PathBuf::from(&output_dir_str);

    let app_data = app_handle.path().app_data_dir().unwrap();
    let covers_path = app_data.join("covers");
    if !covers_path.exists() {
        let _ = fs::create_dir_all(&covers_path);
    }

    let download_result = match task.payload {
        DownloadPayload::AlbumURL(url) => {
            process_album_url(platform, &url, &output_dir_str, &app_handle, &task_id).await
        }
        DownloadPayload::TrackURL(_) => Err("TrackURL extraction not implemented yet".into()),
        DownloadPayload::Single(track) => {
            ytdlp::download_batch(&app_handle, &[track], &output_dir_str, &task_id)
                .await
                .map(|files| (output_dir_path.clone(), files, None))
        }
        DownloadPayload::Batch { tracks } => {
            ytdlp::download_batch(&app_handle, &tracks, &output_dir_str, &task_id)
                .await
                .map(|files| (output_dir_path.clone(), files, None))
        }
    };

    let (target_library_dir, downloaded_files, cover_path) = match download_result {
        Ok(res) => res,
        Err(e) => {
            let _ = app_handle.emit(
                "download-task-failed",
                TaskEvent {
                    task_id: task_id.clone(),
                    message: format!("Download failed: {}", e),
                },
            );
            return Err(e);
        }
    };

    println!("[Queue Worker] Executing shared post-processor tagging and folder allocation operations...");

    let total_tracks = downloaded_files.len();
    let mut new_songs: Vec<Song> = Vec::new();

    for (i, (temp_file, track)) in downloaded_files.iter().enumerate() {
        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.clone(),
                current: i + 1,
                total: total_tracks,
                track_title: track.title.clone(),
            },
        );

        let track_target_dir = if is_album_payload {
            target_library_dir.clone()
        } else {
            target_library_dir
                .join(sanitize_fs(&track.album_artist))
                .join(sanitize_fs(&track.album))
        };

        match process_and_move_track(
            temp_file,
            track,
            &track_target_dir,
            cover_path.as_deref(),
            &covers_path,
            platform.clone(),
        ) {
            Ok(song) => new_songs.push(song),
            Err(err) => {
                eprintln!(
                    "[Post-Processor Error] Mapping failed for {}: {}",
                    track.title, err
                );
            }
        }
    }

    if let Some((first_temp_file, _)) = downloaded_files.first() {
        if let Some(temp_stage_dir) = first_temp_file.parent() {
            if temp_stage_dir.exists() {
                let _ = fs::remove_dir_all(temp_stage_dir);
            }
        }
    }

    if !new_songs.is_empty() {
        println!(
            "[Queue Worker] Inserting {} downloaded songs into the database...",
            new_songs.len()
        );

        let db_state = app_handle.state::<tokio::sync::Mutex<Database>>();
        let db = db_state.lock().await;

        if let Err(e) = db.insert_songs(new_songs).await {
            eprintln!(
                "[Queue Worker] Failed to insert downloaded songs into DB: {}",
                e
            );
        }
    }

    let _ = app_handle.emit(
        "download-task-completed",
        TaskEvent {
            task_id: task_id.clone(),
            message: "Batch pipeline processing complete".into(),
        },
    );

    println!("[Queue Worker] Batch pipeline processing complete.");
    Ok(())
}

/// Handles parsing, directory creation, cover fetching, and batch downloading for Album URLs.
async fn process_album_url(
    platform: Platform,
    url: &str,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<(PathBuf, Vec<(PathBuf, TrackDownload)>, Option<PathBuf>), String> {
    let _ = app_handle.emit(
        "download-task-progress",
        ProgressEvent {
            task_id: task_id.to_string(),
            current: 0,
            total: 0,
            track_title: "Fetching album information...".into(),
        },
    );

    let root_library_dir = PathBuf::from(output_dir);
    let mut target_library_dir = root_library_dir.clone();

    let album_info = match platform {
        Platform::Bandcamp => bandcamp::parse_album(url).await?,
        Platform::Youtube => youtube::parse_album(url).await?,
        Platform::Local => {
            return Err("Local files cannot be processed via remote URL pipelines.".into());
        }
    };

    let _ = app_handle.emit(
        "download-task-progress",
        ProgressEvent {
            task_id: task_id.to_string(),
            current: 0,
            total: 0,
            track_title: format!(
                "Found {} tracks. Preparing directories...",
                album_info.tracks.len()
            ),
        },
    );

    let album_artist_fallback = album_info.album_artist.clone();

    let mut structural_tracks = album_info.tracks.clone();
    for track in &mut structural_tracks {
        track.album_artist = album_artist_fallback.clone();
    }

    let folder_artist = sanitize_fs(&album_artist_fallback);
    let sanitized_album = sanitize_fs(&album_info.title);
    target_library_dir = target_library_dir.join(folder_artist).join(sanitized_album);

    let target_folder_created = target_library_dir != root_library_dir;
    if target_folder_created {
        tokio::fs::create_dir_all(&target_library_dir)
            .await
            .map_err(|e| format!("Failed to create library folder structure: {}", e))?;
    }

    let mut cover_path = None;
    if let Some(cover_url) = &album_info.external_cover_url {
        println!("[Queue Worker] Pre-fetching album artwork: {}", cover_url);

        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.to_string(),
                current: 0,
                total: 0,
                track_title: "Downloading album cover...".into(),
            },
        );

        if let Err(err) = download_album_cover(cover_url, &target_library_dir).await {
            eprintln!("[Queue Worker] Artwork warning (non-fatal): {}", err);
        } else {
            cover_path = Some(target_library_dir.join("cover.jpg"));
        }
    }

    match ytdlp::download_batch(app_handle, &structural_tracks, output_dir, task_id).await {
        Ok(files) => Ok((target_library_dir, files, cover_path)),
        Err(err) => {
            if target_folder_created
                && target_library_dir.exists()
                && fs::remove_dir_all(&target_library_dir).is_ok()
            {
                remove_empty_parents_up_to(&target_library_dir, &root_library_dir);
            }
            Err(err)
        }
    }
}

/// Shared business logic handler that transforms files from unstructured scratch locations
/// to the structured local filesystem library tree.
pub fn process_and_move_track(
    source_file: &Path,
    track: &TrackDownload,
    target_album_dir: &Path,
    cover_path: Option<&Path>,
    covers_dir: &Path,
    platform: Platform,
) -> Result<Song, String> {
    if !source_file.exists() {
        return Err(format!("Source track stream missing: {:?}", source_file));
    }

    let actual_ext = source_file
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("mp3");

    let album_artist = &track.album_artist;

    if let Err(err) = apply_metadata_tags(source_file, track, &album_artist, cover_path, platform) {
        eprintln!(
            "[Metadata Tagging Warning] Non-fatal issue processing ID3 headers: {}",
            err
        );
    }

    let clean_track_name = sanitize_fs(&track.title);
    let final_file_name = match track.track_number {
        Some(num) => format!("{:02} - {}.{}", num, clean_track_name, actual_ext),
        None => format!("{}.{}", clean_track_name, actual_ext),
    };

    let destination_path = target_album_dir.join(final_file_name);

    if !target_album_dir.exists() {
        fs::create_dir_all(target_album_dir)
            .map_err(|e| format!("Failed to create target album dir: {}", e))?;
    }

    fs::rename(source_file, &destination_path)
        .map_err(|e| format!("Failed shifting file from staging into library: {}", e))?;

    let tagged_file = Probe::open(&destination_path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let duration_ms = tagged_file.properties().duration().as_millis() as u64;

    let mut final_cover = cover_path.map(|p| p.to_string_lossy().to_string());

    if final_cover.is_none() {
        if let Some(tag) = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
        {
            if let Some(pic) = tag.pictures().first() {
                let hash_input = format!("{}{}", track.artists.join(", "), track.album);

                let mut hasher = DefaultHasher::new();
                Hash::hash(&hash_input, &mut hasher);
                let hash = format!("{:016x}", hasher.finish());

                let ext = if pic
                    .mime_type()
                    .map_or(false, |m| m.as_str().contains("png"))
                {
                    "png"
                } else {
                    "jpg"
                };
                let full_path = covers_dir.join(format!("{hash}.{ext}"));

                if !full_path.exists() {
                    let _ = fs::write(&full_path, pic.data());
                }
                final_cover = Some(full_path.to_string_lossy().to_string());
            }
        }
    }

    let canonical_track_slug = make_canonical_slug(&album_artist, &track.title);
    let canonical_album_slug = make_canonical_slug(&album_artist, &track.album);
    let quality_tier = determine_quality(actual_ext, &tagged_file);

    Ok(Song {
        id: None,
        path: destination_path.to_string_lossy().to_string(),
        title: track.title.clone(),
        artists: track.artists.clone(),
        album_artist: album_artist.clone(),
        album: track.album.clone(),
        duration_ms,
        track_number: track.track_number,
        genre: track.genres.as_ref().and_then(|g| g.first().cloned()),
        release_year: track.release_year,
        cover_url: final_cover,
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
        source: platform,
        source_url: Some(track.url.clone()),
        source_item_id: track.source_item_id.clone(),
        canonical_track_slug,
        canonical_album_slug,
        quality_tier,
    })
}

fn apply_metadata_tags(
    file_path: &Path,
    track: &TrackDownload,
    album_artist: &str,
    cover_path: Option<&Path>,
    source: Platform,
) -> Result<(), String> {
    let mut tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(existing_tag) => existing_tag,
        None => {
            let primary_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(primary_type));
            tagged_file.primary_tag_mut().unwrap()
        }
    };

    tag.set_title(track.title.clone());
    tag.set_artist(track.artists.join(", "));
    tag.set_album(track.album.clone());

    tag.insert_text(lofty::tag::ItemKey::AlbumArtist, album_artist.to_string());

    if let Some(ref source_id) = track.source_item_id {
        let source_str = match source {
            Platform::Youtube => "YouTube",
            Platform::Bandcamp => "Bandcamp",
            Platform::Local => "Local",
        };

        let tracking_payload = format!("blacktape_source:{}|id:{}", source_str, source_id);
        tag.insert_text(lofty::tag::ItemKey::Comment, tracking_payload);
    }

    if let Some(year) = track.release_year {
        if let Ok(timestamp) = Timestamp::from_str(&year.to_string()) {
            tag.set_date(timestamp);
        }
    }

    if let Some(track_num) = track.track_number {
        tag.set_track(track_num as u32);
    }

    if let Some(path) = cover_path {
        if path.exists() {
            if let Ok(mut img_file) = File::open(path) {
                if let Ok(mut picture) = Picture::from_reader(&mut img_file) {
                    picture.set_pic_type(PictureType::CoverFront);
                    tag.push_picture(picture);
                }
            }
        }
    }

    tag.save_to_path(file_path, WriteOptions::default())
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn download_album_cover(url: &str, output_directory: &Path) -> Result<(), String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Failed to request thumbnail: {}", e))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image bytes: {}", e))?;

    let file_path = output_directory.join("cover.jpg");

    tokio::fs::write(file_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write artwork data: {}", e))?;

    Ok(())
}
