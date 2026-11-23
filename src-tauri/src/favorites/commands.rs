use crate::db::db::Database;
use crate::types::Song;
use tauri::{command, State};
use tokio::sync::Mutex;

#[command]
pub async fn get_favorite_songs(state: State<'_, Mutex<Database>>) -> Result<Vec<Song>, String> {
    let db = state.lock().await;
    db.get_favorite_songs().await.map_err(|e| e.to_string())
}

#[command]
pub async fn get_favorite_ids(state: State<'_, Mutex<Database>>) -> Result<Vec<i64>, String> {
    let db = state.lock().await;
    db.get_favorite_song_ids().await.map_err(|e| e.to_string())
}

#[command]
pub async fn toggle_favorite(
    state: State<'_, Mutex<Database>>,
    song_id: i64,
) -> Result<bool, String> {
    let db = state.lock().await;
    db.toggle_favorite(song_id).await.map_err(|e| e.to_string())
}

#[command]
pub async fn add_favorites(
    state: State<'_, Mutex<Database>>,
    song_ids: Vec<i64>,
) -> Result<(), String> {
    let db = state.lock().await;
    db.add_favorites(&song_ids).await.map_err(|e| e.to_string())
}

#[command]
pub async fn remove_favorites(
    state: State<'_, Mutex<Database>>,
    song_ids: Vec<i64>,
) -> Result<(), String> {
    let db = state.lock().await;
    db.remove_favorites(&song_ids).await.map_err(|e| e.to_string())
}
