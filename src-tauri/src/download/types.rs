use crate::types::Platform;
use serde::Serialize;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize)]
pub struct TrackMetaItem {
    pub title: String,
    pub track_number: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumMetaEvent {
    pub task_id: String,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub tracks: Vec<TrackMetaItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackProgressEvent {
    pub task_id: String,
    pub track_index: usize,
    pub track_title: String,
    pub percent: i32,
}

#[derive(Debug, Clone, Default)]
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
    /// Only the tracks of the album at `url` whose page urls are in `track_urls`.
    AlbumSelection { url: String, track_urls: Vec<String> },
    TrackURL(String),
}

impl DownloadPayload {
    pub fn is_album(&self) -> bool {
        matches!(
            self,
            DownloadPayload::AlbumURL(_) | DownloadPayload::AlbumSelection { .. }
        )
    }
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: String,
    pub platform: Platform,
    pub payload: DownloadPayload,
    pub output_dir: String,
}

pub type DownloadResult = (PathBuf, Vec<(PathBuf, TrackDownload)>, Option<PathBuf>);
