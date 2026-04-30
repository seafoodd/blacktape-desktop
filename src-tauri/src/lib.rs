mod audio;
mod db;
mod discord_presence;
mod lyrics;
mod music;
mod types;

use crate::audio::media_controls::MediaControls;
use crate::audio::player::RepeatMode;
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::lyrics::{fetch_lyrics, LyricsSource};
use crate::types::{Album, ArtistSummary};
use audio::player::AudioPlayer;
use std::sync::Mutex;
use tauri::{command, generate_handler, Listener, Manager, State, WebviewWindow};
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

    let songs = music::scan::scan_music_dir(dir, &covers_path);
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
fn get_is_paused(state: State<Mutex<AudioPlayer>>) -> bool {
    let player = state.lock().unwrap();
    player.is_paused()
}

#[command]
fn toggle(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.toggle();
}

#[command]
fn set_volume(fraction: f32, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.set_volume(fraction);
    println!("SET VOLUME {}", fraction);
}

#[command]
fn get_volume(state: State<Mutex<AudioPlayer>>) -> f32 {
    let mut player = state.lock().unwrap();
    player.get_volume()
}

#[command]
fn fetch_state(state: State<Mutex<AudioPlayer>>) {
    let player = state.lock().unwrap();
    player.emit_state();
}

#[command]
fn toggle_shuffle(state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.toggle_shuffle();
}

#[command]
fn set_repeat_mode(state: State<Mutex<AudioPlayer>>, repeat_mode: RepeatMode) {
    let mut player = state.lock().unwrap();
    println!("Setting repeat mode to: {:?}", repeat_mode);
    player.set_repeat_mode(repeat_mode);
}

#[command]
async fn get_search_suggestions(query: String) -> Vec<String> {
    let mut sugs: Vec<String> = Vec::new();
    sugs.push("suggestion 1".to_string());
    sugs.push("suggestion 2".to_string());
    sugs.push("suggestion 33".to_string());
    sugs
}

#[command]
async fn get_lyrics(
    state: State<'_, tokio::sync::Mutex<Database>>,
    id: i64,
) -> Result<LyricsSource, String> {
    let (artist, title) = {
        let db = state.lock().await;
        let song = db
            .get_song_by_id(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Song not found")?;

        if let (Some(lyrics), Some(source)) = (song.lyrics, song.lyrics_source) {
            if !lyrics.is_empty() {
                let lyrics_source = LyricsSource {
                    lyrics,
                    source,
                };

                return Ok(lyrics_source);
            }
        }
        (song.artist.clone(), song.title.clone())
    };

    let lyrics_source = fetch_lyrics(&artist, &title).await?;

    let db = state.lock().await;
    db.update_song_lyrics(id, lyrics_source.clone())
        .await
        .map_err(|e| e.to_string())?;

    Ok(lyrics_source)
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
        .invoke_handler(generate_handler![
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
            get_artist_albums,
            set_volume,
            get_volume,
            fetch_state,
            toggle_shuffle,
            set_repeat_mode,
            get_lyrics,
            get_search_suggestions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
