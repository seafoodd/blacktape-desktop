use serde::{Deserialize, Serialize};
use crate::audio::player::RepeatMode;

#[derive(Serialize, Debug, Deserialize, Clone, sqlx::FromRow)]
pub struct Song {
    pub id: Option<i64>,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub track_number: Option<i32>,
    pub genre: Option<String>,
    pub release_year: Option<i32>,
    pub cover_url: Option<String>,
    pub external_cover_url: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PlayerState {
    pub current_song: Option<Song>,
    pub is_playing: bool,
    pub progress: f32,
    pub volume: f32,
    pub shuffle_mode: bool,
    pub repeat_mode: RepeatMode,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct ArtistSummary {
    pub name: String,
    pub album_count: i32,
    pub cover_url: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Album {
    pub title: String,
    pub cover_url: Option<String>,
    pub songs: Vec<Song>,
}