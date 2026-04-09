use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Song {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,

    #[serde(skip)]
    pub cover: Option<(Vec<u8>, String)>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayerState {
    pub current_song: Option<Song>,
    pub is_playing: bool,
    pub progress: f32,
}
