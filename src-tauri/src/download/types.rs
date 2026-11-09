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
    /// Length in milliseconds when the platform page/API provides it (parsers fill it; None otherwise).
    pub duration_ms: Option<u64>,
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

/// Where a downloaded single track belongs: the library album and song it stands for (the user picked this
/// result for that missing song, so the names and the number of the album win over the platform's).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TrackPlacement {
    pub album: String,
    pub album_artist: String,
    pub title: String,
    pub track_number: Option<i32>,
}

impl TrackPlacement {
    pub fn apply(&self, track: &mut TrackDownload) {
        track.album = self.album.clone();
        track.album_artist = self.album_artist.clone();
        track.artists = vec![self.album_artist.clone()];
        track.title = self.title.clone();
        track.track_number = self.track_number;
    }
}

#[derive(Debug, Clone)]
pub enum DownloadPayload {
    Single(TrackDownload),
    Batch { tracks: Vec<TrackDownload> },
    AlbumURL(String),
    /// Only the tracks of the album at `url` whose page urls are in `track_urls`.
    AlbumSelection { url: String, track_urls: Vec<String> },
    TrackURL(String),
    /// A single track that is filed into a known album with the given names and number.
    PlacedTrack { url: String, placement: TrackPlacement },
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

#[cfg(test)]
mod placement_tests {
    use super::*;

    fn track() -> TrackDownload {
        TrackDownload {
            url: "u".into(),
            file_name: "f".into(),
            title: "Song (Official Video)".into(),
            artists: vec!["Artist - Topic".into()],
            album_artist: String::new(),
            album: String::new(),
            track_number: None,
            genres: None,
            release_year: None,
            source_item_id: None,
            duration_ms: None,
        }
    }

    #[test]
    fn placement_overrides_names_and_number() {
        let mut t = track();
        TrackPlacement { album: "The Album".into(), album_artist: "Artist".into(), title: "Song".into(), track_number: Some(5) }
            .apply(&mut t);
        assert_eq!((t.album.as_str(), t.album_artist.as_str(), t.title.as_str()), ("The Album", "Artist", "Song"));
        assert_eq!(t.artists, vec!["Artist".to_string()]);
        assert_eq!(t.track_number, Some(5));
    }
}
