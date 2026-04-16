use std::fs;
use std::path::PathBuf;
use crate::types::Song;
use lofty::prelude::*;
use lofty::probe::Probe;
use sha2::{Sha256, Digest};
use walkdir::WalkDir;

pub fn scan_music_dir(dir: String, covers_dir: PathBuf) -> Vec<Song> {
    let mut songs = Vec::new();

    if !covers_dir.exists() {
        fs::create_dir_all(&covers_dir).ok();
    }

    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !["mp3", "flac", "ogg", "wav"].contains(&ext) {
            continue;
        }

        let tagged_file = match Probe::open(path).and_then(|p| p.read()) {
            Ok(f) => f,
            Err(e) => {
                println!("{}", e);
                continue;
            }
        };

        let tag = match tagged_file.primary_tag() {
            Some(t) => t,
            None => continue,
        };

        let mut cover_url = None;

        if let Some(pic) = tag.pictures().first() {
            let data = pic.data();
            let mime = pic.mime_type()
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "image/jpeg".to_string());

            // Fix Cow mismatch: convert both to a standard String for the key
            let artist = tag.artist().map(|s| s.to_string()).unwrap_or_else(|| "Unknown".to_string());
            let album = tag.album().map(|s| s.to_string()).unwrap_or_else(|| "Unknown".to_string());
            let album_key = format!("{}{}", artist, album);

            // Fix Hash Formatting
            let mut hasher = Sha256::new();
            hasher.update(album_key.as_bytes());
            let hash_result = hasher.finalize();
            let hash = hash_result
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();

            let ext = if mime.contains("png") { "png" } else { "jpg" };
            let filename = format!("{}.{}", hash, ext);
            let full_path = covers_dir.join(&filename);

            if !full_path.exists() {
                let _ = fs::write(&full_path, data);
            }

            cover_url = Some(full_path.to_string_lossy().to_string());
        }

        let song = Song {
            id: None,
            path: path.to_string_lossy().to_string(),
            title: tag
                .title()
                .map_or("Unknown Title".to_string(), |s| s.to_string()),
            artist: tag
                .artist()
                .map_or("Unknown Artist".to_string(), |s| s.to_string()),
            album: tag
                .album()
                .map_or("Unknown Album".to_string(), |s| s.to_string()),
            duration_ms: tagged_file.properties().duration().as_millis() as u64,
            track_number: tag.track().map(|n| n as i32),
            genre: tag.genre().map(|g| g.to_string()),
            release_year: tag.date().map(|d| d.year as i32),
            cover_url,
        };
        println!(
            "Scanned song: {:?}, {:?}, {:?}, {:?}, {:?}, {:?}, {:?}",
            song.title, song.artist, song.album, song.duration_ms, song.track_number, song.genre, song.release_year
        );

        songs.push(song);
    }

    songs
}

pub fn get_song_from_path(path: &str) -> Option<Song> {
    // Read metadata using lofty
    println!("get song from path");
    let tagged_file = match Probe::open(path).and_then(|p| p.read()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to read tags for {}: {}", path, e);
            return None;
        }
    };

    // Get the primary tag if available
    let tag = tagged_file.primary_tag();

    // Build Song struct
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
    };

    Some(song)
}
