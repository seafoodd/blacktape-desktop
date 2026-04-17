mod audio;
mod db;
mod discord_presence;
mod music;
mod types;

use crate::audio::media_controls::MediaControls;
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::types::{Album, ArtistSummary};
use audio::player::AudioPlayer;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{command, Listener, Manager, State, WebviewWindow};
use types::Song;

#[command]
async fn scan_music(
    dir: String,
    app: tauri::AppHandle,
    state: State<'_, tokio::sync::Mutex<Database>>,
) -> Result<Vec<Song>, String> {
    if !std::path::Path::new(&dir).exists() {
        return Err("Directory not found. Please check if the path exists".into());
    }

    let app_data = app.path().app_data_dir().unwrap();
    let covers_path = app_data.join("covers");

    let db = state.lock().await;

    let current_db_songs = db.get_all_songs().await.map_err(|e| e.to_string())?;
    let mut ids_to_remove = Vec::new();

    for song in current_db_songs {
        if !std::path::Path::new(&song.path).exists() {
            if let Some(id) = song.id {
                ids_to_remove.push(id);
            }
        }
    }

    if !ids_to_remove.is_empty() {
        println!("Pruning {} missing songs...", ids_to_remove.len());
        db.delete_songs(ids_to_remove)
            .await
            .map_err(|e| e.to_string())?;
    }

    let songs = music::scan::scan_music_dir(dir, covers_path);
    db.insert_songs(songs.clone())
        .await
        .map_err(|e| e.to_string())?;

    let db_songs = db.get_all_songs().await.map_err(|e| e.to_string())?;
    Ok(db_songs)
}
//
// #[command]
// async fn get_library(dir: String, app: tauri::AppHandle, state: State<'_,  tokio::sync::Mutex<Database>>) -> Result<Vec<Song>, String> {
//     let db = state.lock().await;
//     let db_songs = db.get_all_songs().await.map_err(|e| e.to_string())?;
//
//     Ok(db_songs)
// }

#[command]
async fn get_artists(
    state: State<'_, tokio::sync::Mutex<Database>>,
) -> Result<Vec<ArtistSummary>, String> {
    let db = state.lock().await;
    db.get_artists_summary().await.map_err(|e| e.to_string())
}

#[command]
async fn get_artist_albums(
    state: State<'_, tokio::sync::Mutex<Database>>,
    artist_name: &str,
) -> Result<Vec<Album>, String> {
    let db = state.lock().await;
    db.get_artist_albums(artist_name)
        .await
        .map_err(|e| e.to_string())
}

#[command]
async fn start_playback(
    id: i64,
    queue: Vec<i64>,
    history: Vec<i64>,
    db_state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let db = db_state.lock().await;

    let song = {
        db.get_song_by_id(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Song not found in library".to_string())?
    };

    let mut history_songs = VecDeque::new();
    for id in history {
        if let Ok(Some(s)) = db.get_song_by_id(id).await {
            history_songs.push_back(s);
        }
    }

    let mut queue_songs = VecDeque::new();
    for id in queue {
        if let Ok(Some(s)) = db.get_song_by_id(id).await {
            queue_songs.push_back(s);
        }
    }

    let mut player = player_state.lock().map_err(|_| "Player lock poisoned")?;
    player.stop();
    player.set_history(history_songs);
    player.set_queue(queue_songs);
    player.play(song);

    Ok(())
}

#[command]
fn pause(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.pause();
}

#[command]
fn resume(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.resume();
}

#[command]
fn stop(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.stop();
}

#[command]
fn seek(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.seek(fraction);
}

#[command]
fn next(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.next();
    player.resume()
}

#[command]
fn previous(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.previous()
}

#[command]
fn get_position(state: State<Mutex<AudioPlayer>>) -> f32 {
    let player = state.lock().unwrap();
    player.position()
}

#[command]
fn get_is_paused(state: State<Mutex<AudioPlayer>>) {
    let player = state.lock().unwrap();
    player.is_paused();
}

#[command]
fn toggle(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.toggle();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:blacktape.db", get_migrations())
                .build(),
        )
        .manage(Mutex::new(None::<discord_presence::DiscordRpcClient>))
        .setup(|app| {
            let window: WebviewWindow = app
                .get_webview_window("main")
                .expect("failed to get main window");
            let app_handle = app.handle().clone();
            let media_controls = MediaControls::new(&window, app_handle.clone());
            let audio_player = AudioPlayer::new(media_controls, app_handle.clone());
            app.manage(Mutex::new(audio_player));

            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            if !app_dir.exists() {
                std::fs::create_dir_all(&app_dir).expect("failed to create app data directory");
            }
            let db_path = app_dir.join("blacktape.db");
            let db_path_str = db_path.to_str().expect("path is not valid utf-8");
            let db = tauri::async_runtime::block_on(async { Database::new(db_path_str).await });
            app.manage(tokio::sync::Mutex::new(db));

            let register = |event: &str, action: fn(&mut AudioPlayer)| {
                let handle = app_handle.clone();
                app.listen(event, move |_| {
                    let binding = handle.state::<Mutex<AudioPlayer>>();
                    let mut player = binding.lock().expect("Failed to lock audio player");

                    action(&mut *player);
                });
            };
            register("media-resume", AudioPlayer::resume);
            register("media-pause", AudioPlayer::pause);
            register("media-stop", AudioPlayer::stop);
            register("media-next", AudioPlayer::next);
            register("media-previous", AudioPlayer::previous);
            register("media-toggle", AudioPlayer::toggle);

            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            scan_music,
            start_playback,
            pause,
            resume,
            stop,
            seek,
            next,
            previous,
            get_is_paused,
            get_position,
            toggle,
            get_artists,
            get_artist_albums
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
