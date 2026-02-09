use walkdir::WalkDir;
use lofty::probe::Probe;
use lofty::prelude::*;
use std::borrow::Cow;
use crate::types::Song;

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
            Err(_) => continue,
        };

        let tag = match tagged_file.primary_tag() {
            Some(t) => t,
            None => continue,
        };

        let title = tag
            .title()
            .unwrap_or(Cow::Borrowed("Unknown"))
            .to_string();

        let artist = tag
            .artist()
            .unwrap_or(Cow::Borrowed("Unknown"))
            .to_string();

        let album = tag
            .album()
            .unwrap_or(Cow::Borrowed("Unknown"))
            .to_string();

        let cover = tag
            .pictures()
            .first()
            .map(|pic| pic.data().to_vec());

        songs.push(Song {
            path: path.to_string_lossy().to_string(),
            title,
            artist,
            album,
            cover,
        });
    }

    songs
}
