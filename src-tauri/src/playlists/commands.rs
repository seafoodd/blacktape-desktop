use crate::db::db::Database;
use crate::types::{Playlist, Song};
use tauri::{command, State};
use tokio::sync::Mutex;

#[command]
pub async fn create_playlist(
    state: State<'_, Mutex<Database>>,
    name: String,
    description: Option<String>,
    cover_url: Option<String>,
) -> Result<i64, String> {
    let db = state.lock().await;
    db.create_playlist(&name, description.as_deref(), cover_url.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn get_all_playlists(state: State<'_, Mutex<Database>>) -> Result<Vec<Playlist>, String> {
    let db = state.lock().await;
    db.get_all_playlists().await.map_err(|e| e.to_string())
}

#[command]
pub async fn get_playlist_by_id(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
) -> Result<Option<Playlist>, String> {
    let db = state.lock().await;
    db.get_playlist_by_id(playlist_id)
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn update_playlist_details(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
    name: Option<String>,
    description: Option<String>,
    cover_url: Option<String>,
) -> Result<(), String> {
    let db = state.lock().await;
    db.update_playlist_details(
        playlist_id,
        name.as_deref(),
        description.as_deref(),
        cover_url.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[command]
pub async fn delete_playlist(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
) -> Result<(), String> {
    let db = state.lock().await;
    db.delete_playlist(playlist_id)
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn get_playlist_songs(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
) -> Result<Vec<Song>, String> {
    let db = state.lock().await;
    db.get_playlist_songs(playlist_id)
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn add_songs_to_playlist(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
    song_ids: Vec<i64>,
) -> Result<(), String> {
    let db = state.lock().await;
    db.add_songs_to_playlist(playlist_id, &song_ids)
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn remove_songs_from_playlist(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
    positions: Vec<i64>,
) -> Result<(), String> {
    let db = state.lock().await;
    db.remove_songs_from_playlist(playlist_id, &positions)
        .await
        .map_err(|e| e.to_string())
}

// #[command]
// pub async fn reorder_playlist_songs(
//     state: State<'_, Mutex<Database>>,
//     playlist_id: i64,
//     song_ids: Vec<i64>,
// ) -> Result<(), String> {
//     let db = state.lock().await;
//     db.reorder_playlist_songs(playlist_id, &song_ids)
//         .await
//         .map_err(|e| e.to_string())
// }

#[command]
pub async fn reorder_playlist_song(
    state: State<'_, Mutex<Database>>,
    playlist_id: i64,
    from_position: i64,
    to_position: i64,
) -> Result<(), String> {
    let db = state.lock().await;
    db.reorder_playlist_song(playlist_id, from_position, to_position)
        .await
        .map_err(|e| e.to_string())
}
