mod audio;
mod discord_presence;
mod music;
mod types;

use audio::player::AudioPlayer;
use std::sync::Mutex;
use tauri::{command, Listener, Manager, State, WebviewWindow};

use crate::{audio::media_controls::MediaControls, types::Song};

#[command]
fn scan_music(dir: String) -> Vec<types::Song> {
    music::scan::scan_music_dir(dir)
}

#[command]
fn play_song(song: Song, state: State<Mutex<AudioPlayer>>) {
    let mut player = state.lock().unwrap();
    player.play(song.clone());
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
    let player = state.lock().unwrap();
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
        .setup(|app| {
            let window: WebviewWindow = app
                .get_webview_window("main")
                .expect("failed to get main window");
            let app_handle = app.handle().clone();
            let media_controls = MediaControls::new(&window, app_handle.clone());
            let audio_player = AudioPlayer::new(media_controls, app_handle.clone());

            app.manage(Mutex::new(audio_player));
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
