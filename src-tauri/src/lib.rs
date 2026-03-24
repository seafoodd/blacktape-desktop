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
fn resume(state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().resume();
}

#[command]
fn stop(state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().stop();
}

#[command]
fn seek(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().seek(fraction);
}

#[command]
fn get_position(state: State<Mutex<AudioPlayer>>) -> f32 {
    state.lock().unwrap().position()
}

#[command]
fn get_is_paused(state: State<Mutex<AudioPlayer>>) {
    state.lock().unwrap().is_paused();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_media::init())
        .manage(Mutex::new(AudioPlayer::new()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            scan_music,
            play_song,
            pause,
            resume,
            stop,
            seek,
            get_is_paused,
            get_position
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
