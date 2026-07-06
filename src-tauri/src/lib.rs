pub mod audio;
pub mod db;
mod discord_presence;
pub mod download;
mod lyrics;
pub mod music;
mod search;
pub mod types;
pub mod utils;

use crate::audio::media_controls::MediaControls;
use crate::audio::player::RepeatMode;
use crate::db::db::Database;
use crate::db::schema::get_migrations;
use crate::download::{ytdlp, DownloadPayload, DownloadQueue, DownloadTask};
use crate::lyrics::{fetch_lyrics, LyricsSource};
use crate::search::{process_raw_results, SearchSuggestion};
use crate::types::{Album, ArtistSummary, DownloadType, Platform};
use audio::player::AudioPlayer;
use std::error::Error;
use std::sync::Mutex;
use tauri::{
    command, generate_handler, AppHandle, Listener, Manager, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tokio::fs::{read_to_string, File};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::task::JoinSet;
pub use types::Song;
use url::Url;

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

    let songs = music::scan::scan_music_dir(dir, &covers_path);
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

#[command]
async fn cookies_are_ready(
    app: AppHandle,
    window: WebviewWindow,
    payload: String,
) -> Result<(), String> {
    println!("[blacktape::auth] IPC Cookie payload received!");

    if let Err(e) = write_netscape_cookies(&app, &payload).await {
        eprintln!("[blacktape::auth ERROR] Failed to write cookie file: {}", e);
        return Err(e);
    }

    println!("[blacktape::auth] Cookies successfully stored. Closing auth window.");
    let _ = window.close();
    Ok(())
}

#[command]
async fn launch_youtube_login(app: AppHandle) -> Result<(), String> {
    println!("[blacktape::auth] launch_youtube_login command invoked.");

    let login_url_str = "https://accounts.google.com/ServiceLogin?service=youtube";
    let target_url = Url::parse(login_url_str).map_err(|e| e.to_string())?;

    let app_handle_clone = app.clone();

    let builder = WebviewWindowBuilder::new(
        &app,
        "youtube-login",
        WebviewUrl::External(target_url),
    )
        .data_directory(app.path().app_data_dir().unwrap().join("browser-profiles"))
        .title("Blacktape | Sign into YouTube")
        .inner_size(500.0, 600.0)
        .resizable(false)
        .always_on_top(true)
        // 1. Inject a secure window listener that listens ONLY for an internal browser message
        .initialization_script(r#"
        window.addEventListener("message", (event) => {
            if (event.data && event.data.type === "BLACKTAPE_EXTRACT_COOKIES") {
                // Send it back via a standard message structure that Tauri handles internally
                console.log("[blacktape frontend] Extraction triggered. Sending cookies up...");
                window.location.href = "tauri://cookies?data=" + encodeURIComponent(document.cookie);
            }
        });
    "#)
        .on_navigation(move |url| {
            println!("[blacktape::auth] Navigation detected to: {}", url);

            // 2. Catch our custom protocol redirect containing the cookies!
            if url.scheme() == "tauri" && url.host_str() == Some("cookies") {
                let app_handle_task = app_handle_clone.clone();

                // Extract the query parameter from our custom URI redirection
                let query_str = url.query().unwrap_or("");
                let cookies_encoded = query_str.replace("data=", "");
                let clean_cookies = percent_encoding::percent_decode_str(&cookies_encoded)
                    .decode_utf8_lossy()
                    .to_string();

                tauri::async_runtime::spawn(async move {
                    println!("[blacktape::auth] Custom URI intercepted! Writing cookies...");
                    if let Err(e) = write_netscape_cookies(&app_handle_task, &clean_cookies).await {
                        eprintln!("[blacktape::auth ERROR] Failed to write cookie file: {}", e);
                    }

                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        println!("[blacktape::auth] Successfully stored cookies. Closing login window.");
                        let _ = win.close();
                    }
                });
                return false; // Stop navigation here so it doesn't actually try to route to "tauri://cookies"
            }

            let host = url.host_str();
            let path = url.path();

            if host == Some("www.youtube.com") && (path == "/" || path.is_empty()) {
                println!("[blacktape::auth] YouTube landing detected! Checking for extraction trigger...");

                let app_handle_task = app_handle_clone.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        println!("[blacktape::auth] Dispatching collection trigger via postMessage...");
                        // Safely triggers the initialization script handler
                        let _ = win.eval("window.postMessage({ type: 'BLACKTAPE_EXTRACT_COOKIES' }, '*');");
                    }
                });
            }
            true
        });

    let _login_window = builder.build().map_err(|e| e.to_string())?;

    Ok(())
}

async fn write_netscape_cookies(app: &AppHandle, cookies_str: &str) -> Result<(), String> {
    println!(
        "[blacktape::auth] write_netscape_cookies invoked with string length: {}",
        cookies_str.len()
    );

    let mut cookie_file_path = app.path().app_data_dir().map_err(|e| {
        eprintln!("[blacktape::auth ERROR] Failed to get app_data_dir: {}", e);
        e.to_string()
    })?;
    cookie_file_path.push("youtube_cookies.txt");
    println!(
        "[blacktape::auth] Target cookie file path: {:?}",
        cookie_file_path
    );

    let file = File::create(&cookie_file_path).await.map_err(|e| {
        eprintln!(
            "[blacktape::auth ERROR] Failed to create cookie file: {}",
            e
        );
        e.to_string()
    })?;

    let mut writer = BufWriter::new(file);

    writer
        .write_all(b"# Netscape HTTP Cookie File\n")
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(b"# This file was generated by Blacktape. Do not edit.\n\n")
        .await
        .map_err(|e| e.to_string())?;

    let mut cookie_count = 0;
    for cookie in cookies_str.split(';') {
        let parts: Vec<&str> = cookie.trim().splitn(2, '=').collect();
        if parts.len() == 2 {
            let name = parts[0];
            let value = parts[1];
            cookie_count += 1;

            let line = format!(".youtube.com\tTRUE\t/\tTRUE\t0\t{}\t{}\n", name, value);
            writer.write_all(line.as_bytes()).await.map_err(|e| {
                eprintln!("[blacktape::auth ERROR] Failed writing cookie line: {}", e);
                e.to_string()
            })?;
        }
    }
    println!(
        "[blacktape::auth] Parsed and prepared {} cookies to write.",
        cookie_count
    );

    writer.flush().await.map_err(|e| {
        eprintln!("[blacktape::auth ERROR] Failed to flush BufWriter: {}", e);
        e.to_string()
    })?;

    println!(
        "[blacktape::auth SUCCESS] Authenticated yt-dlp cookies saved to: {:?}",
        cookie_file_path
    );
    Ok(())
}

#[command]
async fn check_auth_status(app: AppHandle) -> bool {
    let mut cookie_file = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    cookie_file.push("youtube_cookies.txt");

    // 1. If your exported file doesn't exist, they aren't logged in
    if !cookie_file.exists() {
        return false;
    }

    // 2. Read the exported Netscape file
    let contents = match read_to_string(cookie_file).await {
        Ok(text) => text,
        Err(_) => return false,
    };

    // 3. Verify that your login extraction keys exist in the text file
    // yt-dlp generally needs keys like SID, HSID, or SSID to confirm auth
    let has_sid = contents.contains("SID");
    let has_sapisid = contents.contains("SAPISID");

    has_sid && has_sapisid
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
            get_search_suggestions,
            download,
            launch_youtube_login,
            cookies_are_ready,
            check_auth_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
