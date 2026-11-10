use crate::audio::player::{clamp_volume, lock_player, AudioPlayer, RepeatMode};
use crate::audio::queue_ops;
use crate::db::db::Database;
use crate::Song;
use std::sync::Mutex;
use tauri::{command, State};

#[command]
pub async fn start_playback(
    queue: Vec<i64>,
    current_index: usize,
    db_state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let db = db_state.lock().await;

    let fetched_songs = db
        .get_songs_by_ids(&queue)
        .await
        .map_err(|err| format!("SQLx failed to fetch songs: {:?}", err))?;

    use std::collections::HashMap;
    let song_map: HashMap<i64, Song> = fetched_songs
        .into_iter()
        .filter_map(|s| s.id.map(|id| (id, s)))
        .collect();

    // songs that are no longer in the library are dropped; the start index follows the requested song
    let resolved: Vec<bool> = queue.iter().map(|id| song_map.contains_key(id)).collect();
    let master_songs: Vec<Song> = queue
        .iter()
        .filter_map(|id| song_map.get(id).cloned())
        .collect();

    if master_songs.is_empty() {
        return Err("Queue is empty or songs could not be loaded.".to_string());
    }
    let start = queue_ops::index_after_filter(&resolved, current_index);

    lock_player(&player_state).start_playback(master_songs, start);

    Ok(())
}

#[command]
pub fn play_song_in_queue(
    index: i64,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let mut player = lock_player(&player_state);
    player.play_song_in_queue(index);
    Ok(())
}

/// Called after downloads finished: queued streams of tracks that are in the library now play from the files.
#[command]
pub fn swap_streams_for_downloads(
    songs: Vec<Song>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<usize, String> {
    let mut player = lock_player(&player_state);
    Ok(player.swap_streams_for_downloads(&songs))
}

#[command]
pub fn remove_from_queue(
    positions: Vec<usize>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let mut player = lock_player(&player_state);
    player.remove_from_queue(&positions);
    Ok(())
}

#[command]
pub fn move_in_queue(
    positions: Vec<usize>,
    insert_before: usize,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let mut player = lock_player(&player_state);
    player.move_in_queue(&positions, insert_before);
    Ok(())
}

#[command]
pub fn get_queue(state: State<Mutex<AudioPlayer>>) -> Vec<Song> {
    let player = lock_player(&state);
    player.get_ordered_queue()
}

#[command]
pub async fn add_to_queue(
    song_id: i64,
    db_state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let db = db_state.lock().await;
    let song = db.require_song_by_id(song_id).await?;

    let mut player = lock_player(&player_state);
    player.add_to_queue(song);

    Ok(())
}

#[command]
pub async fn add_next_to_queue(
    song_id: i64,
    db_state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let db = db_state.lock().await;
    let song = db.require_song_by_id(song_id).await?;

    let mut player = lock_player(&player_state);
    player.add_next_to_queue(song);

    Ok(())
}

#[command]
pub fn pause(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.pause();
}

#[command]
pub fn resume(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.resume();
}

#[command]
pub fn stop(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.stop();
}

#[command]
pub fn seek(fraction: f32, state: State<Mutex<AudioPlayer>>) -> Result<(), String> {
    let mut player = lock_player(&state);
    player.seek(fraction)
}

#[command]
pub fn next(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.next();
}

#[command]
pub fn previous(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.previous();
}

#[command]
pub fn get_position(state: State<Mutex<AudioPlayer>>) -> f32 {
    let player = lock_player(&state);
    player.position()
}

#[command]
pub fn get_is_paused(state: State<Mutex<AudioPlayer>>) -> bool {
    let player = lock_player(&state);
    player.is_paused()
}

#[command]
pub fn toggle(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.toggle();
}

#[command]
pub fn set_volume(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.set_volume(clamp_volume(fraction));
}

#[command]
pub fn get_volume(state: State<Mutex<AudioPlayer>>) -> f32 {
    let player = lock_player(&state);
    player.get_volume()
}

#[command]
pub fn fetch_state(state: State<Mutex<AudioPlayer>>) {
    let player = lock_player(&state);
    player.emit_state();
}

#[command]
pub fn toggle_shuffle(state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.toggle_shuffle();
}

#[command]
pub fn set_repeat_mode(repeat_mode: RepeatMode, state: State<Mutex<AudioPlayer>>) {
    let mut player = lock_player(&state);
    player.set_repeat_mode(repeat_mode);
}
