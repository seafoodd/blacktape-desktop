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
    TrackURL(String),
}

impl DownloadPayload {
    pub fn is_album(&self) -> bool {
        matches!(self, DownloadPayload::AlbumURL(_))
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
