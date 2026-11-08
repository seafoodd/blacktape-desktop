//! User-initiated library edits: metadata changes (file tags + DB) and deletion.

use crate::db::db::Database;
use crate::tags::{write_basic_tags, write_basic_tags_replace, BasicTags};
use crate::types::Song;
use std::path::Path;

/// Validated, normalized metadata for one song.
#[derive(Debug, Clone, PartialEq)]
pub struct SongInfo {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artist: String,
    pub track_number: Option<i32>,
    pub genre: Option<String>,
    pub release_year: Option<i32>,
}

pub fn split_artists(raw: &str) -> Vec<String> {
    raw.split(|c| c == ',' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

impl SongInfo {
    /// Normalizes raw form input: trims, applies defaults and drops invalid numbers.
    pub fn from_input(
        title: &str,
        artists: &str,
        album: &str,
        album_artist: &str,
        track_number: Option<i32>,
        genre: Option<&str>,
        release_year: Option<i32>,
    ) -> Result<SongInfo, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("Title cannot be empty".into());
        }
        let artists = split_artists(artists);
        let album = album.trim();
        let album = if album.is_empty() {
            "Unknown Album".to_string()
        } else {
            album.to_string()
        };
        let album_artist = album_artist.trim();
        let album_artist = if album_artist.is_empty() {
            artists
                .first()
                .cloned()
                .unwrap_or_else(|| "Unknown Artist".to_string())
        } else {
            album_artist.to_string()
        };
        Ok(SongInfo {
            title,
            artists,
            album,
            album_artist,
            track_number: track_number.filter(|n| *n > 0),
            genre: genre
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(str::to_string),
            release_year: release_year.filter(|y| (1..=9999).contains(y)),
        })
    }

    /// The song's current metadata, used by bulk edits that only change some fields.
    pub fn from_song(song: &Song) -> SongInfo {
        SongInfo {
            title: song.title.clone(),
            artists: song.artists.clone(),
            album: song.album.clone(),
            album_artist: song.album_artist.clone(),
            track_number: song.track_number,
            genre: song.genre.clone(),
            release_year: song.release_year,
        }
    }
}

/// Writes `info` into the audio file's tags (so a rescan keeps the edit). A missing
/// file is not an error: the DB entry can still be edited.
pub fn write_song_file(song: &Song, info: &SongInfo) -> Result<(), String> {
    write_song_file_with(song, info, write_basic_tags)
}

/// Like `write_song_file`, but swaps in a rewritten copy instead of modifying the
/// file in place, so a player that has the file open is not disturbed.
pub fn write_song_file_replace(song: &Song, info: &SongInfo) -> Result<(), String> {
    write_song_file_with(song, info, write_basic_tags_replace)
}

fn write_song_file_with(
    song: &Song,
    info: &SongInfo,
    write: fn(&Path, &BasicTags) -> Result<(), String>,
) -> Result<(), String> {
    let path = Path::new(&song.path);
    if !path.exists() {
        return Ok(());
    }
    write(
        path,
        &BasicTags {
            title: &info.title,
            artists: &info.artists,
            album: &info.album,
            album_artist: &info.album_artist,
            track_number: info.track_number.map(|n| n as u32),
            genre: info.genre.as_deref(),
            release_year: info.release_year,
        },
    )
}

/// Stores `info` in the database and returns the updated song.
pub async fn update_song_db(db: &Database, id: i64, info: &SongInfo) -> Result<Song, String> {
    db.update_song_info(
        id,
        &info.title,
        &info.artists,
        &info.album,
        &info.album_artist,
        info.track_number,
        info.genre.as_deref(),
        info.release_year,
    )
    .await
    .map_err(|e| e.to_string())?;

    db.require_song_by_id(id).await
}

/// Result of deleting several songs: ids that are gone and per-song failures.
#[derive(Debug, Default)]
pub struct DeleteOutcome {
    pub deleted_ids: Vec<i64>,
    pub failures: Vec<String>,
}

/// Deletes the given songs' files (when `delete_file`) and returns which ids can be
/// removed from the DB. A song whose file cannot be deleted is kept and reported.
pub fn delete_song_files(songs: &[Song], delete_file: bool) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    for song in songs {
        let Some(id) = song.id else { continue };
        if delete_file {
            let path = Path::new(&song.path);
            if path.exists() {
                if let Err(e) = std::fs::remove_file(path) {
                    outcome
                        .failures
                        .push(format!("{}: {}", song.path, e));
                    continue;
                }
            }
            if let Some(parent) = path.parent() {
                // Only removes the folder when it is empty.
                let _ = std::fs::remove_dir(parent);
            }
        }
        outcome.deleted_ids.push(id);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Platform, QualityTier};

    #[test]
    fn from_input_trims_and_defaults() {
        let info = SongInfo::from_input(" T ", "A; B , ,C", " ", "", Some(0), Some("  "), Some(0))
            .unwrap();
        assert_eq!(info.title, "T");
        assert_eq!(info.artists, vec!["A", "B", "C"]);
        assert_eq!(info.album, "Unknown Album");
        assert_eq!(info.album_artist, "A");
        assert_eq!(info.track_number, None);
        assert_eq!(info.genre, None);
        assert_eq!(info.release_year, None);
    }

    #[test]
    fn from_input_rejects_empty_title_and_keeps_valid_numbers() {
        assert!(SongInfo::from_input("  ", "A", "Al", "A", None, None, None).is_err());
        let info =
            SongInfo::from_input("T", "", "Al", "", Some(4), Some(" Rock "), Some(1999)).unwrap();
        assert_eq!(info.album_artist, "Unknown Artist");
        assert_eq!(info.track_number, Some(4));
        assert_eq!(info.genre.as_deref(), Some("Rock"));
        assert_eq!(info.release_year, Some(1999));
        assert!(SongInfo::from_input("T", "", "Al", "", None, None, Some(10000))
            .unwrap()
            .release_year
            .is_none());
    }

    fn song(id: Option<i64>, path: &Path) -> Song {
        Song {
            id,
            path: path.to_string_lossy().into_owned(),
            title: "T".into(),
            artists: vec!["A".into()],
            album_artist: "A".into(),
            album: "Al".into(),
            duration_ms: 0,
            track_number: None,
            genre: None,
            release_year: None,
            cover_url: None,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source: Platform::Local,
            source_url: None,
            source_item_id: None,
            canonical_track_slug: "t".into(),
            canonical_album_slug: "al".into(),
            quality_tier: QualityTier::Low,
        }
    }

    #[test]
    fn delete_song_files_removes_files_and_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir(&album).unwrap();
        let a = album.join("a.mp3");
        let b = album.join("b.mp3");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let songs = vec![song(Some(1), &a), song(Some(2), &b)];

        let outcome = delete_song_files(&songs, true);

        assert!(!a.exists());
        assert!(!b.exists());
        assert_eq!(outcome.deleted_ids, vec![1, 2]);
        assert!(outcome.failures.is_empty());
        assert!(!album.exists());
    }

    #[test]
    fn delete_song_files_keeps_files_when_not_requested() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp3");
        std::fs::write(&a, b"a").unwrap();
        let songs = vec![song(Some(1), &a)];

        let outcome = delete_song_files(&songs, false);

        assert!(a.exists());
        assert_eq!(outcome.deleted_ids, vec![1]);
        assert!(outcome.failures.is_empty());
    }

    #[test]
    fn delete_song_files_returns_id_when_file_already_missing() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone").join("x.mp3");
        let songs = vec![song(Some(7), &missing)];

        let outcome = delete_song_files(&songs, true);

        assert_eq!(outcome.deleted_ids, vec![7]);
        assert!(outcome.failures.is_empty());
    }

    #[test]
    fn delete_song_files_skips_songs_without_id() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("keep.mp3");
        std::fs::write(&f, b"k").unwrap();
        let songs = vec![song(None, &f)];

        let outcome = delete_song_files(&songs, true);

        assert!(outcome.deleted_ids.is_empty());
        assert!(outcome.failures.is_empty());
        assert!(f.exists());
    }

    #[test]
    fn delete_song_files_keeps_folder_with_unrelated_file() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir(&album).unwrap();
        let a = album.join("a.mp3");
        let cover = album.join("cover.jpg");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&cover, b"c").unwrap();
        let songs = vec![song(Some(1), &a)];

        let outcome = delete_song_files(&songs, true);

        assert!(!a.exists());
        assert!(cover.exists());
        assert!(album.exists());
        assert_eq!(outcome.deleted_ids, vec![1]);
    }
}
