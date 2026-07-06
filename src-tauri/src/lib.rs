pub mod audio;
pub mod auth;
pub mod db;
mod discord_presence;
pub mod download;
mod lyrics;
pub mod scan;
mod search;
pub mod types;
pub mod utils;

use crate::audio::commands::{
    fetch_state, get_is_paused, get_position, get_volume, next, pause, previous, resume, seek,
    set_repeat_mode, set_volume, start_playback, stop, toggle, toggle_shuffle,
};
use crate::audio::media_controls::MediaControls;
use crate::auth::youtube::{check_auth_status, cookies_are_ready, launch_youtube_login};
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::download::{ytdlp, DownloadPayload, DownloadQueue, DownloadTask};
use crate::lyrics::{fetch_lyrics, LyricsSource};
use crate::search::{process_raw_results, SearchSuggestion};
use crate::types::{Album, ArtistSummary, DownloadType, Platform};
use audio::player::AudioPlayer;
use std::error::Error;
use std::sync::Mutex;
use tauri::{command, generate_handler, AppHandle, Listener, Manager, State, WebviewWindow};
use tokio::task::JoinSet;
pub use types::Song;

#[command]
async fn scan_music(
    dir: String,
    app: AppHandle,
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

    let songs = scan::scan_music_dir(dir, &covers_path);
    db.insert_songs(songs.clone())
        .await
        .map_err(|e| e.to_string())?;

    let db_songs = db.get_all_songs().await.map_err(|e| e.to_string())?;
    Ok(db_songs)
}

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
async fn get_search_suggestions(query: String, platforms: Vec<Platform>) -> Vec<SearchSuggestion> {
    let mut tasks: JoinSet<Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>>> =
        JoinSet::new();

    for platform in platforms {
        let q = query.clone();
        tasks.spawn(async move {
            match platform {
                Platform::Bandcamp => search::bandcamp::search(&q).await,
                Platform::Youtube => search::youtube::search(&q).await,
            }
        });
    }

    let mut raw_suggestions = Vec::new();
    while let Some(res) = tasks.join_next().await {
        match res {
            Ok(Ok(results)) => raw_suggestions.extend(results),
            Ok(Err(e)) => eprintln!("Platform search error: {e}"),
            Err(e) => eprintln!("Task join error: {e}"),
        }
    }

    process_raw_results(raw_suggestions, &query)
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
                let lyrics_source = LyricsSource { lyrics, source };

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

#[command]
async fn download(
    app_handle: AppHandle,
    queue_state: State<'_, DownloadQueue>,
    platform: Platform,
    download_type: DownloadType,
    url: String,
) -> Result<String, String> {
    let home_dir = app_handle.path().home_dir().map_err(|e| e.to_string())?;
    let download_dir = home_dir.join("blacktape-lib").to_str().unwrap().to_string();

    let payload = match download_type {
        DownloadType::Album => DownloadPayload::AlbumURL(url),
        DownloadType::Track => DownloadPayload::TrackURL(url),
    };

    queue_state
        .tx
        .send(DownloadTask {
            platform,
            payload,
            output_dir: download_dir,
        })
        .map_err(|e| e.to_string())?;

    Ok("Task queued successfully".to_string())
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

            let updater_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = ytdlp::check_and_update(updater_handle).await {
                    eprintln!("[blacktape Error] yt-dlp lifecycle sync failed: {e}");
                }
            });

            let tx = download::init_queue_worker(app.handle().clone());
            app.manage(DownloadQueue { tx });

            let register = |event: &str, action: fn(&mut AudioPlayer)| {
                let handle = app_handle.clone();
                app.listen(event, move |_| {
                    let binding = handle.state::<Mutex<AudioPlayer>>();
                    let mut player = binding.lock().expect("Failed to lock audio player");

                    action(&mut player);
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
            // Player Commands
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
            set_volume,
            get_volume,
            fetch_state,
            toggle_shuffle,
            set_repeat_mode,
            // Scan
            scan_music,
            // DB Queries
            get_artists,
            get_artist_albums,
            get_lyrics,
            // Search
            get_search_suggestions,
            // Download
            download,
            // Auth
            launch_youtube_login,
            cookies_are_ready,
            check_auth_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
