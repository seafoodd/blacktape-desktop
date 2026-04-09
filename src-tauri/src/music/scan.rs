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
            cover: tag.pictures().first().map(|pic| pic.data().to_vec()),
            duration: tagged_file.properties().duration(),
        };
        println!(
            "Scanned song: {:?}, {:?}, {:?}, {:?}",
            song.title, song.artist, song.album, song.duration
        );

        songs.push(song);
    }

    songs
}
