use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Song {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub track_number: Option<i32>,
    pub genre: Option<String>,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,

    #[serde(skip)]
    pub cover: Option<(Vec<u8>, String)>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayerState {
    pub current_song: Option<Song>,
    pub is_playing: bool,
    pub progress: f32,
}
