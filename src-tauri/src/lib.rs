mod audio;
mod music;
mod types;

use audio::player::AudioPlayer;
use std::sync::Mutex;
use tauri::{command, State};

#[command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[command]
fn scan_music(dir: String) -> Vec<types::Song> {
    music::scan::scan_music_dir(dir)
}

#[command]
fn play_song(path: String, state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().play(path);
}

#[command]
fn pause(state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().pause();
}

#[command]
fn stop(state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().stop();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_media::init())
        .manage(Mutex::new(AudioPlayer::new()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet, scan_music, play_song, pause, stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
