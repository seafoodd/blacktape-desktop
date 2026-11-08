//! Writing user-edited metadata back into the audio file's tags.
//!
//! The library scanner re-reads tags and overwrites the DB, so edits made in
//! the app must also land in the file or the next scan would revert them.

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, ItemKey, Tag};
use std::path::Path;

pub struct BasicTags<'a> {
    pub title: &'a str,
    pub artists: &'a [String],
    pub album: &'a str,
    pub album_artist: &'a str,
    pub track_number: Option<u32>,
    pub genre: Option<&'a str>,
    pub release_year: Option<i32>,
}

pub fn write_basic_tags(path: &Path, info: &BasicTags) -> Result<(), String> {
    let mut tagged = Probe::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?
        .read()
        .map_err(|e| format!("Failed to read tags of {}: {}", path.display(), e))?;

    if tagged.primary_tag().is_none() && tagged.first_tag().is_none() {
        let tag_type = tagged.file_type().primary_tag_type();
        tagged.insert_tag(Tag::new(tag_type));
    }

    let tag = match tagged.primary_tag_mut() {
        Some(t) => t,
        None => tagged
            .first_tag_mut()
            .ok_or_else(|| "File has no writable tag".to_string())?,
    };

    tag.set_title(info.title.to_string());
    tag.set_artist(info.artists.join(", "));
    tag.set_album(info.album.to_string());
    tag.insert_text(ItemKey::AlbumArtist, info.album_artist.to_string());
    match info.track_number {
        Some(n) => tag.set_track(n),
        None => tag.remove_track(),
    }
    match info.genre {
        Some(g) if !g.is_empty() => tag.set_genre(g.to_string()),
        _ => tag.remove_genre(),
    }
    match info.release_year {
        Some(y) if (1..=9999).contains(&y) => tag.set_date(Timestamp {
            year: y as u16,
            ..Default::default()
        }),
        _ => tag.remove_date(),
    }

    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|e| format!("Failed to write tags to {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::file::TaggedFileExt;
    use lofty::tag::Accessor;

    /// Builds a minimal valid 16-bit mono PCM WAV containing `seconds` of silence.
    fn silent_wav_bytes(sample_rate: u32, seconds: u32) -> Vec<u8> {
        let channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = sample_rate * block_align as u32;
        let data_len = byte_rate * seconds;

        let mut buf = Vec::with_capacity(44 + data_len as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_len).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.resize(44 + data_len as usize, 0);
        buf
    }

    #[test]
    fn write_basic_tags_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        std::fs::write(&path, silent_wav_bytes(8000, 1)).unwrap();

        let artists = vec!["X".to_string(), "Y".to_string()];
        write_basic_tags(
            &path,
            &BasicTags {
                title: "T",
                artists: &artists,
                album: "Al",
                album_artist: "X",
                track_number: Some(5),
                genre: None,
                release_year: None,
            },
        )
        .unwrap();

        let tagged = Probe::open(&path).unwrap().read().unwrap();
        let tag = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .expect("tag should exist after write");

        assert_eq!(tag.title().as_deref(), Some("T"));
        assert_eq!(tag.artist().as_deref(), Some("X, Y"));
        assert_eq!(tag.album().as_deref(), Some("Al"));
        assert_eq!(tag.track(), Some(5));
        assert_eq!(tag.get_string(ItemKey::AlbumArtist), Some("X"));
    }

    #[test]
    fn write_basic_tags_errors_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does_not_exist.wav");

        let artists = vec!["X".to_string()];
        let result = write_basic_tags(
            &path,
            &BasicTags {
                title: "T",
                artists: &artists,
                album: "Al",
                album_artist: "X",
                track_number: None,
                genre: None,
                release_year: None,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn write_basic_tags_roundtrip_genre_and_year() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        std::fs::write(&path, silent_wav_bytes(8000, 1)).unwrap();

        let artists = vec!["X".to_string()];
        write_basic_tags(
            &path,
            &BasicTags {
                title: "T",
                artists: &artists,
                album: "Al",
                album_artist: "X",
                track_number: None,
                genre: Some("Rock"),
                release_year: Some(1999),
            },
        )
        .unwrap();

        let tagged = Probe::open(&path).unwrap().read().unwrap();
        let tag = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .expect("tag should exist after write");
        assert_eq!(tag.genre().as_deref(), Some("Rock"));
        assert_eq!(tag.date().map(|d| d.year), Some(1999));

        write_basic_tags(
            &path,
            &BasicTags {
                title: "T",
                artists: &artists,
                album: "Al",
                album_artist: "X",
                track_number: None,
                genre: None,
                release_year: None,
            },
        )
        .unwrap();

        let tagged = Probe::open(&path).unwrap().read().unwrap();
        let tag = tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .expect("tag should exist after second write");
        assert!(tag.genre().is_none());
        assert!(tag.date().is_none());
    }
}
