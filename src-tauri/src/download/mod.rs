use crate::types::Platform;
use crate::utils::{remove_empty_parents_up_to, sanitize};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, Tag, TagExt};
use std::fs::{self, File};
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;
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
            println!("RECEIVED TASK {task:?}");
            let output_dir = task.output_dir.clone();
            let app_handle = app_handle.clone();

            let result: Result<String, String> = async move {
                let root_library_dir = PathBuf::from(&output_dir);
                let mut target_library_dir = PathBuf::from(&output_dir);
                let mut cover_path = None;
                let mut target_folder_created = false;

                let downloaded_files: Vec<(PathBuf, TrackDownload)> = match task.payload {
                    DownloadPayload::AlbumURL(url) => {
                        let album_info = match task.platform {
                            Platform::Bandcamp => bandcamp::parse_album(&url).await?,
                            Platform::Youtube => youtube::parse_album(&url).await?,
                        };

                        if !album_info.artists.is_empty() {
                            let primary_artist = sanitize(&album_info.artists[0]);
                            let sanitized_album = sanitize(&album_info.title);
                            target_library_dir = target_library_dir.join(&primary_artist).join(&sanitized_album);
                        }

                        tokio::fs::create_dir_all(&target_library_dir).await.map_err(|e| {
                            format!("Failed to create library folder structure: {}", e)
                        })?;
                        target_folder_created = true;

                        // Pre-fetch album cover if it exists
                        if let Some(ref cover_url) = album_info.external_cover_url {
                            println!("[Queue Worker] Pre-fetching album artwork: {}", cover_url);
                            if let Err(err) = download_album_cover(cover_url, &target_library_dir).await {
                                eprintln!("[Queue Worker] Artwork warning (non-fatal): {}", err);
                            } else {
                                cover_path = Some(target_library_dir.join("cover.jpg"));
                            }
                        }

                        let download_res = match task.platform {
                            Platform::Youtube | Platform::Bandcamp => {
                                ytdlp::download_batch(&app_handle, &album_info.tracks, &output_dir).await
                            }
                        };

                        match download_res {
                            Ok(data) => data,
                            Err(err) => {
                                if target_folder_created && target_library_dir.exists() {
                                    let _ = fs::remove_dir_all(&target_library_dir);
                                    remove_empty_parents_up_to(&target_library_dir, &root_library_dir);
                                }
                                return Err(err);
                            }
                        }
                    }
                    DownloadPayload::TrackURL(_url) => {
                        return Err("TrackURL extraction not implemented yet".into());
                    }
                    DownloadPayload::Single(track) => {
                        // Single download fallback handling using YouTube
                        ytdlp::download_batch(&app_handle, &[track], &output_dir).await?
                    }
                    DownloadPayload::Batch { tracks } => {
                        ytdlp::download_batch(&app_handle, &tracks, &output_dir).await?
                    }
                };

                println!("[Queue Worker] Executing shared post-processor tagging and folder allocation operations...");

                for (temp_file, track) in downloaded_files.iter() {
                    if let Err(err) = process_and_move_track(temp_file, track, &target_library_dir, cover_path.as_deref()) {
                        eprintln!("[Post-Processor Error] Mapping failed for {}: {}", track.title, err);
                    }
                }

                if let Some((first_temp_file, _)) = downloaded_files.first() {
                    if let Some(temp_stage_dir) = first_temp_file.parent() {
                        if temp_stage_dir.exists() {
                            let _ = fs::remove_dir_all(temp_stage_dir);
                        }
                    }
                }

                Ok("Batch pipeline processing complete".to_string())
            }
                .await;

            match result {
                Ok(_) => println!("[Queue Worker] Successfully processed download task."),
                Err(e) => eprintln!("[Queue Worker] Task failure error branch: {}", e),
            }
        }
    });

    tx
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
        .and_then(|e| e.to_str())
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
        fs::create_dir_all(target_album_dir).map_err(|e| e.to_string())?;
    }

    // Shift file out of staging to destination
    fs::rename(source_file, &destination_path)
        .map_err(|e| format!("Failed shifting file from staging into library: {}", e))?;

    Ok(())
}

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

    // Embed cover artwork if provided and found on disk
    if let Some(path) = cover_path {
        if path.exists() {
            if let Ok(mut img_file) = File::open(path) {
                if let Ok(picture) = Picture::from_reader(&mut img_file) {
                    let mut final_pic = picture;
                    final_pic.set_pic_type(PictureType::CoverFront);
                    tag.push_picture(final_pic);
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
    let mut file = tokio::fs::File::create(file_path)
        .await
        .map_err(|e| format!("Failed to create cover file: {}", e))?;

    file.write_all(&bytes)
        .await
        .map_err(|e| format!("Failed to write artwork data: {}", e))?;

    Ok(())
}
