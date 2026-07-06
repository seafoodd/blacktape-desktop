use crate::types::Platform;
use crate::utils::{remove_empty_parents_up_to, sanitize};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, Tag, TagExt};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::AppHandle;
use tokio::sync::mpsc;

pub mod bandcamp;
pub mod youtube;
pub mod ytdlp;

#[derive(Debug, Clone)]
pub struct TrackDownload {
    pub url: String,
    pub file_name: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub track_number: Option<i32>,
    pub genres: Option<Vec<String>>,
    pub release_year: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct AlbumDownload {
    pub title: String,
    pub artists: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct DownloadTask {
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
    println!("RECEIVED TASK {task:?}");
    let output_dir_path = PathBuf::from(&task.output_dir);

    // 1. Determine target directories and fetch/download files based on payload type
    let (target_library_dir, downloaded_files, cover_path) = match task.payload {
        DownloadPayload::AlbumURL(url) => {
            process_album_url(task.platform, &url, &task.output_dir, &app_handle).await?
        }
        DownloadPayload::TrackURL(_) => {
            return Err("TrackURL extraction not implemented yet".into());
        }
        DownloadPayload::Single(track) => {
            let files = ytdlp::download_batch(&app_handle, &[track], &task.output_dir).await?;
            (output_dir_path.clone(), files, None)
        }
        DownloadPayload::Batch { tracks } => {
            let files = ytdlp::download_batch(&app_handle, &tracks, &task.output_dir).await?;
            (output_dir_path.clone(), files, None)
        }
    };

    println!("[Queue Worker] Executing shared post-processor tagging and folder allocation operations...");

    // 2. Process, tag, and move each downloaded track to its final destination
    for (temp_file, track) in &downloaded_files {
        if let Err(err) =
            process_and_move_track(temp_file, track, &target_library_dir, cover_path.as_deref())
        {
            eprintln!(
                "[Post-Processor Error] Mapping failed for {}: {}",
                track.title, err
            );
        }
    }

    // 3. Cleanup temporary staging directory
    if let Some((first_temp_file, _)) = downloaded_files.first() {
        if let Some(temp_stage_dir) = first_temp_file.parent() {
            if temp_stage_dir.exists() {
                let _ = fs::remove_dir_all(temp_stage_dir);
            }
        }
    }

    println!("[Queue Worker] Batch pipeline processing complete.");
    Ok(())
}

/// Handles parsing, directory creation, cover fetching, and batch downloading for Album URLs.
async fn process_album_url(
    platform: Platform,
    url: &str,
    output_dir: &str,
    app_handle: &AppHandle,
) -> Result<(PathBuf, Vec<(PathBuf, TrackDownload)>, Option<PathBuf>), String> {
    let root_library_dir = PathBuf::from(output_dir);
    let mut target_library_dir = root_library_dir.clone();

    let album_info = match platform {
        Platform::Bandcamp => bandcamp::parse_album(url).await?,
        Platform::Youtube => youtube::parse_album(url).await?,
    };

    // Construct structured artist/album path if artist data is available
    if let Some(primary_artist) = album_info.artists.first() {
        let primary_artist = sanitize(primary_artist);
        let sanitized_album = sanitize(&album_info.title);
        target_library_dir = target_library_dir
            .join(primary_artist)
            .join(sanitized_album);
    }

    let target_folder_created = target_library_dir != root_library_dir;
    if target_folder_created {
        tokio::fs::create_dir_all(&target_library_dir)
            .await
            .map_err(|e| format!("Failed to create library folder structure: {}", e))?;
    }

    // Pre-fetch album cover if provided
    let mut cover_path = None;
    if let Some(cover_url) = &album_info.external_cover_url {
        println!("[Queue Worker] Pre-fetching album artwork: {}", cover_url);
        if let Err(err) = download_album_cover(cover_url, &target_library_dir).await {
            eprintln!("[Queue Worker] Artwork warning (non-fatal): {}", err);
        } else {
            cover_path = Some(target_library_dir.join("cover.jpg"));
        }
    }

    // Download tracks and handle cleanup if the batch download fails
    match ytdlp::download_batch(app_handle, &album_info.tracks, output_dir).await {
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
) -> Result<(), String> {
    if !source_file.exists() {
        return Err(format!("Source track stream missing: {:?}", source_file));
    }

    // Fallback detection logic to preserve file compression format container extensions
    let actual_ext = source_file
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("mp3");

    // Inject tag mappings via Lofty before moving files
    if let Err(err) = apply_metadata_tags(source_file, track, cover_path) {
        eprintln!(
            "[Metadata Tagging Warning] Non-fatal issue processing ID3 headers: {}",
            err
        );
    }

    // Build standard, audio file names
    let clean_track_name = sanitize(&track.title);
    let final_file_name = match track.track_number {
        Some(num) => format!("{:02} - {}.{}", num, clean_track_name, actual_ext),
        None => format!("{}.{}", clean_track_name, actual_ext),
    };

    let destination_path = target_album_dir.join(final_file_name);

    // Commit directory safety check
    if !target_album_dir.exists() {
        fs::create_dir_all(target_album_dir)
            .map_err(|e| format!("Failed to create target album dir: {}", e))?;
    }

    // Shift file out of staging to destination
    fs::rename(source_file, &destination_path)
        .map_err(|e| format!("Failed shifting file from staging into library: {}", e))?;

    Ok(())
}

/// Reads the downloaded file and injects metadata tags (ID3, Vorbis, etc.) using Lofty.
fn apply_metadata_tags(
    file_path: &Path,
    track: &TrackDownload,
    cover_path: Option<&Path>,
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

    if let Some(year) = track.release_year {
        if let Ok(timestamp) = Timestamp::from_str(&year.to_string()) {
            tag.set_date(timestamp);
        }
    }

    if let Some(track_num) = track.track_number {
        tag.set_track(track_num as u32);
    }

    // Embed cover artwork if found on disk
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

/// Downloads an image from a URL and saves it as `cover.jpg` in the specified directory.
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
