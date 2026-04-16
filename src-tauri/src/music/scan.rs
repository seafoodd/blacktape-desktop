use crate::types::Song;
use lofty::prelude::*;
use lofty::probe::Probe;
use walkdir::WalkDir;

pub fn scan_music_dir(dir: String) -> Vec<Song> {
    let mut songs = Vec::new();

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

        let song = Song {
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
            cover: tag.pictures().first().map(|pic| {
                let mime = pic
                    .mime_type()
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "image/jpeg".to_string());

                (pic.data().to_vec(), mime)
            }),
            duration_ms: tagged_file.properties().duration().as_millis() as u64,
            track_number: None,
            genre: None,
            release_year: None,
            cover_url: None,
        };
        println!(
            "Scanned song: {:?}, {:?}, {:?}, {:?}",
            song.title, song.artist, song.album, song.duration_ms
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
        cover: tag.and_then(|t| {
            t.pictures().first().map(|pic| {
                let mime = pic
                    .mime_type()
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_else(|| "image/jpeg".to_string());
                (pic.data().to_vec(), mime)
            })
        }),
    };

    Some(song)
}
