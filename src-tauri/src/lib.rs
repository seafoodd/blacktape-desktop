mod audio;
mod db;
mod discord_presence;
mod music;
mod types;

use crate::audio::media_controls::MediaControls;
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use audio::player::AudioPlayer;
use std::sync::Mutex;
use tauri::{command, Listener, Manager, State, WebviewWindow};
use types::Song;

#[command]
async fn scan_music(dir: String, state: State<'_, tokio::sync::Mutex<Database>>) -> Result<Vec<Song>, String> {
    let songs = music::scan::scan_music_dir(dir);
    let songs_to_insert = songs.clone();

    let db = state.lock().await;
    db.insert_songs(songs_to_insert)
        .await
        .map_err(|e| e.to_string())?;


    let cleaned_songs = songs
        .into_iter()
        .map(|mut song| {
            song.cover = None;
            song
        })
        .collect();

    Ok(cleaned_songs)
}

#[command]
fn play_song(song: Song, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.play_from_path(&song.path);
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
        .setup(|app| {
            let window: WebviewWindow = app
                .get_webview_window("main")
                .expect("failed to get main window");
            let app_handle = app.handle().clone();
            let media_controls = MediaControls::new(&window, app_handle.clone());
            let audio_player = AudioPlayer::new(media_controls, app_handle.clone());
            app.manage(Mutex::new(audio_player));

            let app_dir = app.path().app_data_dir().expect("failed to get app data dir");
            if !app_dir.exists() {
                std::fs::create_dir_all(&app_dir).expect("failed to create app data directory");
            }
            let db_path = app_dir.join("blacktape.db");
            let db_path_str = db_path.to_str().expect("path is not valid utf-8");
            let db = tauri::async_runtime::block_on(async {
                Database::new(db_path_str).await
            });
            app.manage(tokio::sync::Mutex::new(db));

            match discord_presence::DiscordRpcClient::new() {
                Ok(client) => {
                    app.manage(Mutex::new(client));
                    println!("Discord RPC managed");
                }
                Err(e) => {
                    eprintln!("Discord RPC disabled: {}", e);
                }
            }

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
            play_song,
            pause,
            resume,
            stop,
            seek,
            get_is_paused,
            get_position,
            toggle
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
