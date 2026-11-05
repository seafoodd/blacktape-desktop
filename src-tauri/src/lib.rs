pub mod audio;
pub mod auth;
pub mod db;
mod discord_presence;
pub mod download;
mod lyrics;
mod playlists;
pub mod scan;
mod search;
pub mod types;
pub mod utils;

use crate::audio::commands::{
    add_next_to_queue, add_to_queue, fetch_state, get_is_paused, get_position, get_volume, next,
    pause, play_song_in_queue, previous, resume, seek, set_repeat_mode, set_volume, start_playback,
    stop, toggle, toggle_shuffle,
};
use crate::audio::media_controls::MediaControls;
use crate::auth::youtube::{check_auth_status, cookies_are_ready, launch_youtube_login};
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::download::{ytdlp, DownloadPayload, DownloadQueue, DownloadTask, TaskRegistry};
use crate::lyrics::{fetch_lyrics, fetch_lyrics_from_provider, LyricsSource};
use crate::playlists::commands::{
    add_songs_to_playlist, create_playlist, delete_playlist, get_all_playlists, get_playlist_by_id,
    get_playlist_songs, remove_songs_from_playlist, reorder_playlist_song, update_playlist_details,
};
use crate::search::{process_raw_results, SearchSuggestion};
use crate::types::{Album, ArtistSummary, DownloadType, Platform};
use audio::player::AudioPlayer;
use std::error::Error;
use std::sync::Mutex;
use tauri::{
    command, generate_handler, AppHandle, Emitter, Listener, Manager, State, WebviewWindow,
};
use tokio::task::JoinSet;
pub use types::Song;

pub struct AuthLock(pub tokio::sync::Mutex<()>);

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
                _ => Ok(Vec::new()),
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
    let (artist, title, album, duration_ms) = {
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

        let artist_name =
            if !song.album_artist.trim().is_empty() && song.album_artist != "Unknown Artist" {
                song.album_artist.clone()
            } else if let Some(first_artist) = song.artists.first() {
                first_artist.clone()
            } else {
                song.album_artist.clone()
            };

        (
            artist_name,
            song.title.clone(),
            song.album.clone(),
            song.duration_ms,
        )
    };

    let fetch_handle = tokio::spawn(async move {
        fetch_lyrics(&artist, &title, Some(&album), Some(duration_ms)).await
    });

    let lyrics_source = match fetch_handle.await {
        Ok(result) => result?,
        Err(e) => {
            eprintln!("Lyrics fetch task panicked or cancelled: {e}");
            return Err("Failed to fetch lyrics due to an internal error".to_string());
        }
    };

    let db = state.lock().await;
    db.update_song_lyrics(id, lyrics_source.clone())
        .await
        .map_err(|e| e.to_string())?;

    Ok(lyrics_source)
}

#[command]
async fn refetch_lyrics(
    state: State<'_, tokio::sync::Mutex<Database>>,
    id: i64,
    provider: Option<String>,
) -> Result<LyricsSource, String> {
    let (artist, title, album, duration_ms) = {
        let db = state.lock().await;
        let song = db
            .get_song_by_id(id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("Song not found")?;

        let artist_name =
            if !song.album_artist.trim().is_empty() && song.album_artist != "Unknown Artist" {
                song.album_artist.clone()
            } else if let Some(first_artist) = song.artists.first() {
                first_artist.clone()
            } else {
                song.album_artist.clone()
            };

        (
            artist_name,
            song.title.clone(),
            song.album.clone(),
            song.duration_ms,
        )
    };

    let fetch_handle = tokio::spawn(async move {
        if let Some(ref p) = provider {
            if !p.trim().is_empty() && p != "auto" {
                fetch_lyrics_from_provider(p, &artist, &title, Some(&album), Some(duration_ms))
                    .await
            } else {
                fetch_lyrics(&artist, &title, Some(&album), Some(duration_ms)).await
            }
        } else {
            fetch_lyrics(&artist, &title, Some(&album), Some(duration_ms)).await
        }
    });

    let lyrics_source = match fetch_handle.await {
        Ok(result) => result?,
        Err(e) => {
            eprintln!("Lyrics refetch task panicked or cancelled: {e}");
            return Err("Failed to refetch lyrics due to an internal error".to_string());
        }
    };

    let db = state.lock().await;
    db.update_song_lyrics(id, lyrics_source.clone())
        .await
        .map_err(|e| e.to_string())?;

    Ok(lyrics_source)
}

#[command]
async fn save_lyrics(
    state: State<'_, tokio::sync::Mutex<Database>>,
    id: i64,
    lyrics: String,
    source: Option<String>,
) -> Result<LyricsSource, String> {
    let lyrics_source = LyricsSource {
        lyrics,
        source: source.unwrap_or_else(|| "Custom".to_string()),
    };

    let db = state.lock().await;
    db.update_song_lyrics(id, lyrics_source.clone())
        .await
        .map_err(|e| e.to_string())?;

    Ok(lyrics_source)
}

#[command]
async fn delete_lyrics(
    state: State<'_, tokio::sync::Mutex<Database>>,
    id: i64,
) -> Result<(), String> {
    let db = state.lock().await;
    db.delete_song_lyrics(id).await.map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
async fn download(
    app_handle: AppHandle,
    queue_state: State<'_, DownloadQueue>,
    task_registry: State<'_, TaskRegistry>,
    auth_lock: State<'_, AuthLock>,
    id: String,
    platform: Platform,
    download_type: DownloadType,
    url: String,
    artist_name: Option<String>,
    title: Option<String>,
    cover_url: Option<String>,
) -> Result<String, String> {
    if platform == Platform::Youtube {
        let res = launch_youtube_login(app_handle.clone(), false, auth_lock).await;
        println!("launch_youtube_login res: {res:?}");
        if res.is_err() {
            eprintln!("[blacktape:lib.rs ERR] auth_window_closed");
            return Err("auth_window_closed".to_string());
        }
    }

    let home_dir = app_handle.path().home_dir().map_err(|e| e.to_string())?;
    let download_dir = home_dir.join("blacktape-lib").to_str().unwrap().to_string();

    let initial_artist = artist_name.unwrap_or_else(|| "Unknown Artist".to_string());
    let initial_title = title.unwrap_or_else(|| "Download".to_string());
    let is_track = download_type == DownloadType::Track;

    let initial_state = crate::download::ActiveDownloadState {
        id: id.clone(),
        platform: format!("{:?}", platform),
        download_type: format!("{:?}", download_type),
        url: url.clone(),
        artist_name: initial_artist,
        album_title: if is_track {
            "Singles".to_string()
        } else {
            initial_title.clone()
        },
        cover_url,
        status: "downloading".to_string(),
        current: 0,
        total: if is_track { 1 } else { 0 },
        overall_percent: 0,
        message: "Starting download...".to_string(),
        tracks: if is_track {
            vec![crate::download::TrackProgressState {
                title: initial_title,
                track_number: Some(1),
                percent: 0,
                status: "downloading".to_string(),
            }]
        } else {
            vec![]
        },
    };

    task_registry.register_task(initial_state).await;

    let payload = match download_type {
        DownloadType::Album => DownloadPayload::AlbumURL(url),
        DownloadType::Playlist => return Err("Playlist downloads unimplemented.".into()),
        DownloadType::Track => DownloadPayload::TrackURL(url),
    };

    queue_state
        .tx
        .send(DownloadTask {
            id,
            platform,
            payload,
            output_dir: download_dir,
        })
        .map_err(|e| e.to_string())?;

    Ok("Task queued successfully".to_string())
}

#[command]
async fn cancel_download(
    app_handle: AppHandle,
    task_registry: State<'_, TaskRegistry>,
    task_id: String,
) -> Result<(), String> {
    println!("[Cancel] Cancelling download task: {}", task_id);
    task_registry.mark_cancelled(&task_id).await;
    task_registry.kill_pids_for_task(&task_id).await;

    if let Some(info) = task_registry.remove_task(&task_id).await {
        let registry_clone = task_registry.inner().clone();
        let task_id_clone = task_id.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            registry_clone.kill_pids_for_task(&task_id_clone).await;

            if let Some(temp_dir) = info.temp_dir {
                println!("[Cancel Cleanup] Deleting temp dir: {:?}", temp_dir);
                let mut deleted = false;
                for attempt in 0..25 {
                    if !temp_dir.exists() {
                        deleted = true;
                        break;
                    }
                    if let Err(e) = crate::utils::force_remove_dir_all(&temp_dir) {
                        eprintln!(
                            "[Cancel Cleanup] Retrying deleting temp dir {:?} (attempt {}): {e}",
                            temp_dir, attempt
                        );
                        registry_clone.kill_pids_for_task(&task_id_clone).await;
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    } else {
                        deleted = true;
                        println!(
                            "[Cancel Cleanup] Successfully deleted temp dir: {:?}",
                            temp_dir
                        );
                        break;
                    }
                }
                if !deleted {
                    eprintln!(
                        "[Cancel Cleanup] Failed to delete temp dir after retries: {:?}",
                        temp_dir
                    );
                }
            }

            if let (Some(target_dir), Some(root_dir)) = (info.target_dir, info.root_dir) {
                if target_dir.exists() {
                    let _ = crate::utils::force_remove_dir_all(&target_dir);
                    crate::utils::remove_empty_parents_up_to(&target_dir, &root_dir);
                }
            }
        });
    }

    let _ = app_handle.emit(
        "download-task-cancelled",
        serde_json::json!({ "task_id": task_id }),
    );
    Ok(())
}

#[command]
async fn get_active_downloads(
    task_registry: State<'_, TaskRegistry>,
) -> Result<Vec<crate::download::ActiveDownloadState>, String> {
    Ok(task_registry.get_all_states().await)
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

            if let Ok(home) = app.path().home_dir() {
                let temp_dir = home.join("blacktape-lib").join(".temp");
                let _ = std::fs::create_dir_all(&temp_dir);
                let _ = crate::utils::set_hidden(&temp_dir, true);
                if let Ok(entries) = std::fs::read_dir(&temp_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            let _ = crate::utils::force_remove_dir_all(&path);
                        } else {
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
            }

            app.manage(TaskRegistry::default());
            let tx = download::init_queue_worker(app.handle().clone());
            app.manage(DownloadQueue { tx });

            app.manage(AuthLock(tokio::sync::Mutex::new(())));

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
            add_to_queue,
            add_next_to_queue,
            play_song_in_queue,
            // Scan
            scan_music,
            // DB Queries
            get_artists,
            get_artist_albums,
            get_lyrics,
            refetch_lyrics,
            save_lyrics,
            delete_lyrics,
            // Search
            get_search_suggestions,
            // Download
            download,
            cancel_download,
            get_active_downloads,
            // Auth
            launch_youtube_login,
            cookies_are_ready,
            check_auth_status,
            // Playlists
            create_playlist,
            get_all_playlists,
            get_playlist_by_id,
            update_playlist_details,
            delete_playlist,
            get_playlist_songs,
            add_songs_to_playlist,
            remove_songs_from_playlist,
            reorder_playlist_song,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
