pub mod audio;
pub mod auth;
pub mod db;
mod discord_presence;
pub mod download;
mod lyrics;
mod playlists;
pub mod scan;
pub mod song_edit;
mod preview;
mod search;
pub mod soundcloud;
pub mod tags;
mod thumbs;
pub mod trash;
pub mod types;
pub mod utils;

use crate::audio::commands::{
    add_next_to_queue, add_to_queue, fetch_state, get_is_paused, get_position, get_volume,
    move_in_queue, next, pause, play_song_in_queue, previous, remove_from_queue, resume, seek,
    set_repeat_mode, set_volume, start_playback, stop, toggle, toggle_shuffle,
};
use crate::audio::media_controls::MediaControls;
use crate::auth::youtube::{check_auth_status, cookies_are_ready, launch_youtube_login};
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::download::{ytdlp, DownloadPayload, DownloadQueue, DownloadTask, TaskRegistry};
use crate::lyrics::{fetch_lyrics, fetch_lyrics_from_provider, LyricsSource};
use crate::playlists::commands::{
    add_songs_to_playlist, create_playlist, delete_playlist, get_all_playlists, get_playlist_by_id,
    get_playlist_songs, move_playlist_songs, remove_songs_from_playlist, reorder_playlist_song,
    update_playlist_details,
};
use crate::search::full::{ArtistProfile, FullSearchPage, SearchCursor, SearchKind};
use crate::search::SearchSuggestion;
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

    let songs = scan::scan_music_dir(dir.clone(), &covers_path);
    db.insert_songs(songs.clone())
        .await
        .map_err(|e| e.to_string())?;
    // The trash folder lives inside the library dir, so the backend needs to know it (D11).
    db.set_app_setting(trash::LIBRARY_DIR_KEY, &dir)
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
                Platform::Youtube => search::search_youtube_with_fallback(&q).await,
                Platform::Soundcloud => search::soundcloud::search(&q).await,
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

    search::ranking::rank(raw_suggestions, &query)
}

#[command]
async fn search_full(
    query: String,
    kind: SearchKind,
    platforms: Vec<Platform>,
    cursor: Option<SearchCursor>,
) -> Result<FullSearchPage, String> {
    search::full::search_full(&query, kind, &platforms, cursor).await
}

#[command]
async fn get_artist_profile(platform: Platform, url: String) -> Result<ArtistProfile, String> {
    search::full::artist_profile(platform, &url).await
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

/// Pushes edited metadata to the player (current song, queue, media controls).
fn refresh_player_songs(player_state: &Mutex<AudioPlayer>, songs: &[Song]) {
    if let Ok(mut player) = player_state.lock() {
        for song in songs {
            player.refresh_song_metadata(song);
        }
    }
}

/// Writes edited metadata into the song's file tags, then the database (the DB is
/// untouched if the file write fails). If the song is playing right now its file is
/// released first and playback resumes from the same position afterwards: rewriting
/// tags under an open decoder otherwise makes the track stop.
async fn save_song_with_player(
    db: &Database,
    player: &Mutex<AudioPlayer>,
    id: i64,
    info: &song_edit::SongInfo,
) -> Result<Song, String> {
    let song = db.require_song_by_id(id).await?;
    // Preferred: write a copy and swap it in. The player keeps reading the old file
    // handle, so playback is not touched at all and no lock is needed.
    if let Err(e) = song_edit::write_song_file_replace(&song, info) {
        eprintln!("[tag edit] atomic replace failed ({e}); falling back to in-place write");
        write_song_in_place(&song, player, id, info)?;
    }
    song_edit::update_song_db(db, id, info).await
}

fn write_song_in_place(
    song: &Song,
    player: &Mutex<AudioPlayer>,
    id: i64,
    info: &song_edit::SongInfo,
) -> Result<(), String> {
    {
        // The lock is held for the whole write so the player's background watchers
        // cannot react to the temporarily empty queue.
        let mut p = player
            .lock()
            .map_err(|_| "Player lock poisoned".to_string())?;
        // Preferred: keep playing from an in-memory copy (no gap). If that is not
        // possible, fall back to closing the file and resuming afterwards.
        let suspended = match p.move_song_to_memory(id) {
            Ok(()) => None,
            Err(e) => {
                eprintln!("[tag edit] in-memory handover failed ({e}); falling back");
                p.release_song_file(id)
            }
        };
        let result = song_edit::write_song_file(song, info);
        if let Some(s) = suspended {
            p.restore_song_file(s);
        }
        result
    }
}

/// Saves edited metadata of one song into the file's tags and the database.
#[command]
async fn update_song_info(
    state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
    id: i64,
    title: String,
    artists: String,
    album: String,
    album_artist: String,
    track_number: Option<i32>,
    genre: Option<String>,
    release_year: Option<i32>,
) -> Result<(), String> {
    let info = song_edit::SongInfo::from_input(
        &title,
        &artists,
        &album,
        &album_artist,
        track_number,
        genre.as_deref(),
        release_year,
    )?;

    let updated = {
        let db = state.lock().await;
        save_song_with_player(&db, &player_state, id, &info).await?
    };
    refresh_player_songs(&player_state, &[updated]);
    Ok(())
}

/// Changes album-level fields for every given song (title, artists and track
/// numbers are kept). `genre` / `release_year` of `None` keep each song's own value.
#[command]
async fn update_album_info(
    state: State<'_, tokio::sync::Mutex<Database>>,
    player_state: State<'_, Mutex<AudioPlayer>>,
    ids: Vec<i64>,
    album: String,
    album_artist: String,
    genre: Option<String>,
    release_year: Option<i32>,
) -> Result<(), String> {
    let mut updated = Vec::with_capacity(ids.len());
    let mut failures = Vec::new();
    {
        let db = state.lock().await;
        for id in ids {
            let song = db.require_song_by_id(id).await?;
            let info = song_edit::SongInfo::from_input(
                &song.title,
                &song.artists.join(", "),
                &album,
                &album_artist,
                song.track_number,
                genre.as_deref().or(song.genre.as_deref()),
                release_year.or(song.release_year),
            )?;
            match save_song_with_player(&db, &player_state, id, &info).await {
                Ok(s) => updated.push(s),
                Err(e) => failures.push(format!("{}: {}", song.title, e)),
            }
        }
    }
    refresh_player_songs(&player_state, &updated);

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Failed to update {} track(s):\n{}",
            failures.len(),
            failures.join("\n")
        ))
    }
}

/// Removes songs from the library. With `delete_file` the audio files are also
/// deleted from disk (and their folders, if that leaves them empty); otherwise only
/// the database entries go away (a later rescan may add them back). Songs whose
/// file cannot be deleted stay in the library and are reported in the error.
#[command]
async fn delete_songs(
    state: State<'_, tokio::sync::Mutex<Database>>,
    ids: Vec<i64>,
    delete_file: bool,
) -> Result<(), String> {
    let db = state.lock().await;
    let songs = db
        .get_songs_by_ids(&ids)
        .await
        .map_err(|e| e.to_string())?;

    let outcome = song_edit::delete_song_files(&songs, delete_file);
    db.delete_songs(outcome.deleted_ids)
        .await
        .map_err(|e| e.to_string())?;

    if outcome.failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Could not delete {} file(s):\n{}",
            outcome.failures.len(),
            outcome.failures.join("\n")
        ))
    }
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
        // thumb://localhost/<encoded path>?s=<px> -> downscaled cover (see thumbs.rs)
        .register_asynchronous_uri_scheme_protocol("thumb", |ctx, request, responder| {
            let cache_dir = ctx
                .app_handle()
                .path()
                .app_cache_dir()
                .ok()
                .map(|d| d.join("thumbs"));
            let uri = request.uri().clone();
            thumbs::spawn_job(move || {
                responder.respond(thumbs::handle(cache_dir, uri.path(), uri.query()));
            });
        })
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
            app.manage(preview::commands::PreviewManager::default());
            preview::commands::spawn_watcher(app_handle.clone());

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
            trash::spawn_auto_clean(app_handle.clone());

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
            // Streaming preview
            preview::commands::preview_play,
            preview::commands::preview_prefetch,
            preview::commands::preview_toggle,
            preview::commands::preview_seek,
            preview::commands::preview_stop,
            preview::commands::preview_get_state,
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
            remove_from_queue,
            move_in_queue,
            // Scan
            scan_music,
            // DB Queries
            get_artists,
            get_artist_albums,
            get_lyrics,
            refetch_lyrics,
            save_lyrics,
            delete_lyrics,
            delete_songs,
            update_album_info,
            update_song_info,
            // Search
            get_search_suggestions,
            search_full,
            get_artist_profile,
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
            move_playlist_songs,
            // Trash
            trash::trash_songs,
            trash::list_trash,
            trash::restore_trash,
            trash::delete_trash,
            trash::empty_trash,
            trash::get_trash_settings,
            trash::set_trash_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
