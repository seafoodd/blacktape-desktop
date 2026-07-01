use crate::download::ytdlp::download_batch;
use crate::types::Platform;
use crate::utils::sanitize;
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::fs::File;
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
                let tracks_to_download = match task.payload {
                    DownloadPayload::Single(track) => vec![track],
                    DownloadPayload::Batch { tracks } => tracks,
                    DownloadPayload::AlbumURL(url) => {
                        let album_info = match task.platform {
                            Platform::Bandcamp => bandcamp::parse_album(&url).await?,
                            Platform::Youtube => youtube::parse_album(&url).await?,
                        };

                        let target_dir = if !album_info.artists.is_empty() {
                            let primary_artist = sanitize(&album_info.artists[0]);
                            let sanitized_album = sanitize(&album_info.title);

                            // You can dynamically switch "Albums" to "EPs" or "Singles" here if your metadata tracks it
                            PathBuf::from(&output_dir)
                                .join(&primary_artist)
                                .join(&sanitized_album)
                        } else {
                            PathBuf::from(output_dir.clone())
                        };

                        tokio::fs::create_dir_all(&target_dir).await.map_err(|e| {
                            format!("Failed to create library folder structure: {}", e)
                        })?;

                        if let Some(ref cover_url) = album_info.external_cover_url {
                            println!("[Queue Worker] Pre-fetching album artwork: {}", cover_url);
                            if let Err(err) = download_album_cover(cover_url, &target_dir).await {
                                eprintln!("[Queue Worker] Artwork warning (non-fatal): {}", err);
                            }
                        }

                        album_info.tracks
                    }
                    DownloadPayload::TrackURL(_url) => {
                        return Err("TrackURL extraction not implemented yet".into());
                    }
                };

                println!(
                    "[Queue Worker] Sending {} tracks to backend engine...",
                    tracks_to_download.len()
                );
                download_batch(&app_handle, &tracks_to_download, &output_dir).await?;

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

pub async fn download_album_cover(url: &str, output_directory: &Path) -> Result<(), String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("Failed to request thumbnail: {}", e))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read image bytes: {}", e))?;

    let file_path = output_directory.join("cover.jpg");
    let mut file = File::create(file_path)
        .await
        .map_err(|e| format!("Failed to create cover file: {}", e))?;

    file.write_all(&bytes)
        .await
        .map_err(|e| format!("Failed to write artwork data: {}", e))?;

    Ok(())
}
