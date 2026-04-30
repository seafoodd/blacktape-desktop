use crate::types::Song;
use lofty::prelude::*;
use lofty::probe::Probe;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

pub fn scan_music_dir(dir: String, covers_dir: &PathBuf) -> Vec<Song> {
    let mut songs = Vec::new();

    if !covers_dir.exists() {
        fs::create_dir_all(covers_dir).ok();
    }

    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if !["mp3", "flac", "ogg", "wav", "m4a", "aiff"].contains(&ext) {
            continue;
        }

        let tagged_file = match Probe::open(path).and_then(Probe::read) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        let tag = tagged_file.primary_tag().or(tagged_file.first_tag());

        let title = tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown Track")
                    .to_string()
            });

        let artist = tag
            .and_then(|t| t.artist().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let album = tag
            .and_then(Accessor::album)
            .map_or_else(|| "Unknown Album".to_string(), |s| s.to_string());

        let mut cover_url = None;

        if let Some(parent) = path.parent() {
            let common_names = [
                "cover.jpg",
                "cover.png",
                "folder.jpg",
                "front.jpg",
                "front.png",
            ];

            for name in common_names {
                let potential_cover = parent.join(name);
                if potential_cover.exists() {
                    cover_url = Some(potential_cover.to_string_lossy().to_string());
                    break;
                }
            }
        }

        if cover_url.is_none() {
            if let Some(t) = tag {
                if let Some(pic) = t.pictures().first() {
                    let data = pic.data();
                    let mime = pic.mime_type().map(lofty::picture::MimeType::as_str).unwrap_or("image/jpeg");

                    let album_key = format!("{artist}{album}");

                    let mut hasher = Sha256::new();
                    hasher.update(album_key.as_bytes());
                    let hash_result = hasher.finalize();
                    let hash = hash_result
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>();

                    let pic_ext = if mime.contains("png") { "png" } else { "jpg" };
                    let filename = format!("{hash}.{pic_ext}");
                    let full_path = covers_dir.join(&filename);

                    if !full_path.exists() {
                        let _ = fs::write(&full_path, data);
                    }
                    cover_url = Some(full_path.to_string_lossy().to_string());
                }
            }
        }

        let song = Song {
            id: None,
            path: path.to_string_lossy().to_string(),
            title,
            artist,
            album,
            duration_ms: tagged_file.properties().duration().as_millis() as u64,
            track_number: tag.and_then(|t| t.track()).map(|n| n as i32),
            genre: tag.and_then(|t| t.genre()).map(|g| g.to_string()),
            release_year: tag.and_then(|t| t.date()).map(|d| d.year as i32),
            cover_url,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
        };
        println!(
            "Scanned song: {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
            song.title,
            song.artist,
            song.album,
            song.duration_ms,
            song.track_number,
            song.genre,
            song.release_year
        );

        songs.push(song);
    }

    songs
}

pub fn get_song_from_path(path: &str) -> Option<Song> {
    println!("get song from path");
    let tagged_file = match Probe::open(path).and_then(|p| p.read()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to read tags for {path}: {e}");
            return None;
        }
    };

    let tag = tagged_file.primary_tag();

    let song = Song {
        id: None,
        path: path.to_string(),
        title: tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Title".into()),
        artist: tag
            .and_then(|t| t.artist().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Artist".into()),
        album: tag
            .and_then(|t| t.album().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Album".into()),
        duration_ms: tagged_file.properties().duration().as_millis() as u64,
        track_number: None,
        genre: None,
        release_year: None,
        cover_url: None,
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
    };

    Some(song)
}
