use crate::audio::player::{AudioPlayer, RepeatMode};
use crate::db::db::Database;
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
    let mut master_songs = Vec::new();
    for id in queue {
        if let Ok(Some(s)) = db.get_song_by_id(id).await {
            master_songs.push(s);
        }
    }

    if master_songs.is_empty() {
        return Err("Queue is empty or songs could not be loaded".to_string());
    }

    let mut player = player_state.lock().map_err(|_| "Player lock poisoned")?;
    player.start_playback(master_songs, current_index);
    Ok(())
}

#[command]
pub fn pause(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.pause();
}

#[command]
pub fn resume(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.resume();
}

#[command]
pub fn stop(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.stop();
}

#[command]
pub fn seek(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.seek(fraction);
}

#[command]
pub fn next(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.next();
}

#[command]
pub fn previous(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.previous();
}

#[command]
pub fn get_position(state: State<Mutex<AudioPlayer>>) -> f32 {
    let player = state.lock().unwrap();
    player.position()
}

#[command]
pub fn get_is_paused(state: State<Mutex<AudioPlayer>>) -> bool {
    let player = state.lock().unwrap();
    player.is_paused()
}

#[command]
pub fn toggle(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.toggle();
}

#[command]
pub fn set_volume(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.set_volume(fraction);
}

#[command]
pub fn get_volume(state: State<Mutex<AudioPlayer>>) -> f32 {
    let mut player = state.lock().unwrap();
    player.get_volume()
}

#[command]
pub fn fetch_state(state: State<Mutex<AudioPlayer>>) {
    let player = state.lock().unwrap();
    player.emit_state();
}

#[command]
pub fn toggle_shuffle(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.toggle_shuffle();
}

#[command]
pub fn set_repeat_mode(state: State<Mutex<AudioPlayer>>, repeat_mode: RepeatMode) {
    let mut player = state.lock().unwrap();
    println!("Setting repeat mode to: {:?}", repeat_mode);
    player.set_repeat_mode(repeat_mode);
}
