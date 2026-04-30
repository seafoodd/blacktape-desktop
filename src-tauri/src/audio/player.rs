use crate::discord_presence::DiscordRpcClient;
use crate::{
    audio::media_controls::MediaControls,
    types::{PlayerState, Song},
};
use rand::rng;
use rand::seq::SliceRandom;
use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait},
    DeviceId, DeviceIdError,
};
use rodio::{Decoder, MixerDeviceSink, Player, Source};
use serde::{Deserialize, Serialize};
use souvlaki::MediaMetadata;
use std::time::Instant;
use std::{fs::File, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    Track,
    Queue,
}

pub struct AudioPlayer {
    sink: MixerDeviceSink,
    player: Player,
    duration: Option<Duration>,
    media_controls: MediaControls,
    current_song: Option<Song>,
    handle: AppHandle,
    queue: Vec<Song>,
    current_device_id: Option<DeviceId>,
    shuffle_mode: bool,
    repeat_mode: RepeatMode,
    play_order: Option<Vec<usize>>,
    cursor: Option<usize>,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls, handle: AppHandle) -> Self {
        let sink = Self::create_sink().expect("Failed to open default audio stream");
        let player = Player::connect_new(sink.mixer());

        Self::spawn_device_watcher(handle.clone());
        Self::spawn_transition_watcher(handle.clone());

        Self::spawn_cover_listener(&handle);

        let initial_device_id = Self::get_default_device_id().ok();

        Self {
            sink,
            player,
            duration: None,
            media_controls,
            current_song: None,
            handle,
            queue: Vec::new(),
            current_device_id: initial_device_id,
            shuffle_mode: false,
            repeat_mode: RepeatMode::Off,
            play_order: None,
            cursor: None,
        }
    }

    fn create_sink() -> Result<MixerDeviceSink, String> {
        rodio::DeviceSinkBuilder::from_default_device()
            .map_err(|_| "No audio device found".to_string())?
            .open_stream()
            .map_err(|e| e.to_string())
    }

    fn spawn_device_watcher(handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;

                let system_default_id = Self::get_default_device_id().ok();

                if let Ok(mut player) = handle.state::<Mutex<AudioPlayer>>().lock() {
                    if let Some(new_id) = system_default_id {
                        if player.current_device_id.as_ref() != Some(&new_id) {
                            println!(
                                "New default audio device detected. Switching from {:?} to: {:?}",
                                player.current_device_id, new_id
                            );
                            let _ = player.reconnect_default_device();
                        }
                    }
                }
            }
        });
    }

    fn spawn_transition_watcher(handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut sleep_time: Duration = Duration::ZERO;
            let mut is_next = false;

            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let state = handle.state::<Mutex<AudioPlayer>>();

                let needs_emit = {
                    let mut player = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);

                    if player.is_paused() {
                        false
                    } else {
                        let current_pos = player.player.get_pos();
                        let mut advanced = false;

                        if let Some(duration) = player.duration {
                            if duration.saturating_sub(current_pos) <= Duration::from_millis(250) {
                                println!("start seamless transition");

                                let Some(next_cursor) = player.get_next_cursor() else {
                                    player.stop();
                                    return;
                                };

                                is_next = player.advance_to_next_in_queue(next_cursor);
                                println!("seamless transition IS NEXT: {is_next}");

                                sleep_time = duration.saturating_sub(current_pos);
                                advanced = true;
                            }
                        }
                        advanced
                    }
                };

                if needs_emit {
                    tokio::time::sleep(sleep_time).await;

                    let mut player = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);

                    if !is_next {
                        player.stop();
                    }

                    player.emit_state();
                    player.update_discord_song();
                    player.update_current_metadata();
                }
            }
        });
    }

    fn get_default_device_id() -> Result<DeviceId, DeviceIdError> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .ok_or(DeviceIdError::BackendSpecific {
                err: cpal::BackendSpecificError {
                    description: "No default output device found".to_string(),
                },
            })?;

        device.id()
    }

    pub fn play(&mut self, song: Song) {
        println!("Playing {}, {:#?}", song.title, song.duration_ms);
        let path = &song.path;
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Could not open file {path}: {e}");
                return;
            }
        };
        let source = match Decoder::try_from(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Failed to decode audio for {path}: {e}");
                return;
            }
        };
        self.duration = source.total_duration();

        self.player.clear();
        self.player.append(source);

        self.player.play();
        self.current_song = Some(song);

        self.update_current_metadata();
        self.media_controls
            .play()
            .expect("Failed to resume media controls state");
        self.emit_state();
        self.update_discord_song();
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.media_controls
            .pause()
            .expect("Failed to pause media controls");
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song();
    }

    pub fn resume(&mut self) {
        if self.player.len() < 1 {
            if let Some(song) = self.current_song.clone() {
                return self.play(song);
            }
        }
        self.player.play();
        self.media_controls
            .play()
            .expect("Failed to resume media controls state");
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song();
    }

    pub fn stop(&mut self) {
        self.player.pause();
        self.player.stop();
        self.media_controls
            .stop()
            .expect("Failed to stop media controls state");
        self.duration = None;
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song();
    }

    pub fn toggle(&mut self) {
        if self.is_paused() {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn add_to_queue(&mut self, song: Song) {
        self.queue.push(song);
        let new_idx = self.queue.len() - 1;

        if let Some(ref mut order) = self.play_order {
            order.push(new_idx);
        } else {
            self.play_order = Some(vec![new_idx]);
            self.cursor = Some(0);
        }
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    pub fn get_queue(&self) -> Vec<Song> {
        self.queue.clone()
    }

    /// Advances the player to the next song in the queue.
    ///
    /// Returns `true` if the player advanced, or `false` if the queue was empty.
    fn advance_to_next_in_queue(&mut self, next_cursor: usize) -> bool {
        self.cursor = Some(next_cursor);

        if let Some(next_song) = self.get_song_at_cursor() {
            self.buffer_next_silent(&next_song);
            self.current_song = Some(next_song.clone());
            self.duration = Some(Duration::from_millis(next_song.duration_ms));
            true
        } else {
            false
        }
    }

    pub fn start_playback(&mut self, songs: Vec<Song>, current_index: usize) {
        let new_play_order: Vec<usize> = (0..songs.len()).collect();

        self.queue = songs;
        self.play_order = Some(new_play_order);
        self.cursor = Some(current_index);

        if self.shuffle_mode {
            self.apply_shuffle_to_queue();
        }
        self.stop();
        if let (Some(order), Some(cursor)) = (&self.play_order, self.cursor) {
            if let Some(&song_idx) = order.get(cursor) {
                if let Some(song) = self.queue.get(song_idx).cloned() {
                    self.play(song);
                }
            }
        }
    }

    fn get_song_at_cursor(&self) -> Option<Song> {
        let order = self.play_order.as_ref()?;
        let cursor = self.cursor?;
        let song_idx = order.get(cursor)?;
        self.queue.get(*song_idx).cloned()
    }

    fn get_next_cursor(&self) -> Option<usize> {
        let order = self.play_order.as_ref()?;
        let current_cursor = self.cursor?;

        if self.repeat_mode == RepeatMode::Track {
            return Some(current_cursor);
        }

        let next_cursor = current_cursor + 1;

        if next_cursor >= order.len() {
            if self.repeat_mode == RepeatMode::Queue {
                Some(0)
            } else {
                None
            }
        } else {
            Some(next_cursor)
        }
    }

    pub fn toggle_shuffle(&mut self) {
        if self.shuffle_mode {
            if let (Some(ref mut order), Some(cursor)) = (&mut self.play_order, self.cursor) {
                let current_actual_idx = order[cursor];
                order.sort_unstable();
                self.cursor = Some(current_actual_idx);
            }
            self.shuffle_mode = false;
        } else {
            self.shuffle_mode = true;
            self.apply_shuffle_to_queue();
        }
        self.emit_state();
    }

    pub fn set_repeat_mode(&mut self, repeat_mode: RepeatMode) {
        self.repeat_mode = repeat_mode;
    }

    fn apply_shuffle_to_queue(&mut self) {
        if let (Some(ref mut order), Some(cursor)) = (&mut self.play_order, self.cursor) {
            let current_idx = order.remove(cursor);
            let mut rng = rng();
            order.shuffle(&mut rng);
            order.insert(0, current_idx);
            self.cursor = Some(0);
            println!("SHSHSHSHSHSHS, {order:?}");
        }
    }

    fn update_current_metadata(&mut self) {
        let Some(song) = &self.current_song else {
            return;
        };

        let formatted_uri: String;

        let uri_ref = if let Some(path) = &song.cover_url {
            formatted_uri = Self::format_cover_path(path);
            Some(formatted_uri.as_str())
        } else {
            None
        };

        self.media_controls.update_metadata(MediaMetadata {
            title: Some(&song.title),
            artist: Some(&song.artist),
            album: Some(&song.album),
            duration: Some(Duration::from_millis(song.duration_ms)),
            cover_url: uri_ref,
        });
    }

    fn buffer_next_silent(&mut self, song: &Song) {
        let now = Instant::now();
        println!("Silent buffer: {}, {:?}", song.title, song.id);
        if let Ok(file) = File::open(&song.path) {
            if let Ok(source) = Decoder::try_from(file) {
                self.player.append(source);
            }
        }

        println!("BUFFERING TOOK: {:#?}", now.elapsed());
    }

    pub fn next(&mut self) {
        self.player.skip_one();

        let Some(order) = &self.play_order else {
            return;
        };
        let Some(current_cursor) = self.cursor else {
            return;
        };

        let mut next_cursor = current_cursor + 1;

        if next_cursor >= order.len() {
            if self.repeat_mode == RepeatMode::Queue || self.repeat_mode == RepeatMode::Track {
                next_cursor = 0;
            } else {
                self.emit_state();
                self.update_current_metadata();
                return;
            }
            println!("REPEATING 0, {}, {}", next_cursor, order.len());
            self.cursor = Some(next_cursor);
        }

        if !self.advance_to_next_in_queue(next_cursor) {
            self.stop();
        }
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song();
    }

    pub fn previous(&mut self) {
        if self.player.get_pos() > Duration::from_secs(3) {
            if let Some(current) = self.get_song_at_cursor() {
                self.play(current);
                return;
            }
        }

        let Some(current_cursor) = self.cursor else {
            return;
        };

        if current_cursor > 0 {
            self.cursor = Some(current_cursor - 1);
            if let Some(prev_song) = self.get_song_at_cursor() {
                self.play(prev_song);
            }
        } else if let Some(current) = self.get_song_at_cursor() {
            self.play(current);
        }
    }

    pub fn seek(&mut self, fraction: f32) {
        let Some(duration) = self.duration else {
            return;
        };
        let target = duration.mul_f32(fraction);
        let remaining = duration.saturating_sub(target);

        println!("PLAYER {}", self.player.len());

        if self.player.len() < 1 {
            if let Some(song) = self.current_song.clone() {
                return self.play(song);
            }
        }

        if remaining < Duration::new(1, 0) {
            let Some(next_cursor) = self.get_next_cursor() else {
                return self.next();
            };

            self.player.skip_one();
            self.advance_to_next_in_queue(next_cursor);
            self.emit_state();
            self.update_discord_song();
            self.update_current_metadata();
            return;
        }

        if let Err(e) = self.player.try_seek(target) {
            eprintln!("Seek failed: {e:?}");
        }
        println!("seeking: {target:?}, REMAININGGGGGGG {remaining:?}, duration: {duration:?}");
        self.update_discord_song();
        self.update_current_metadata();
    }

    pub fn position(&self) -> f32 {
        if let Some(duration) = self.duration {
            let pos = self.player.get_pos();
            return (pos.as_secs_f32() / duration.as_secs_f32()).min(1.0);
        }
        0.0
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn emit_state(&self) {
        let state = PlayerState {
            current_song: self.current_song.clone(),
            is_playing: !self.is_paused(),
            progress: self.position(),
            volume: self.player.volume(),
            shuffle_mode: self.shuffle_mode,
            repeat_mode: self.repeat_mode,
        };

        if let Some(ref song) = state.current_song {
            println!(
                "emitted {}, progress: {}, is_playing: {}",
                song.title, state.progress, state.is_playing
            );
        }

        self.handle.emit("player-state", state).ok();
    }

    // fn cover_file_uri(song: &Song) -> Option<String> {
    //     song.cover.as_ref().map(|(bytes, mime)| {
    //         let ext = match mime.as_str() {
    //             "image/png" => "png",
    //             "image/jpeg" | "image/jpg" => "jpg",
    //             "image/webp" => "webp",
    //             _ => "img",
    //         };
    //
    //         let mut temp_path = std::env::temp_dir();
    //         temp_path.push("blacktape");
    //
    //         if let Some(parent) = temp_path.parent() {
    //             fs::create_dir_all(parent).ok();
    //         }
    //
    //         temp_path.push(format!("current_song_cover.{}", ext));
    //
    //         // write the actual bytes
    //         fs::write(&temp_path, bytes).ok()?;
    //
    //         let path_str = temp_path.to_string_lossy();
    //
    //         #[cfg(target_os = "windows")]
    //         let cover_path = format!("file://{}", path_str.replace('/', "\\"));
    //         #[cfg(not(target_os = "windows"))]
    //         let cover_path = format!("file://{}", path_str);
    //
    //         Some(cover_path)
    //     })?
    // }

    fn format_cover_path(path: &str) -> String {
        #[cfg(target_os = "windows")]
        let cover_path = format!("file://{}", path.replace('/', "\\"));
        #[cfg(not(target_os = "windows"))]
        let cover_path = format!("file://{}", path);

        cover_path
    }

    fn update_discord_song(&self) {
        let song_data = match &self.current_song {
            Some(song) => (song.clone(), self.duration),
            None => return,
        };

        let pos_ms = i64::try_from(self.player.get_pos().as_millis()).unwrap_or(0);
        let is_paused = self.is_paused();
        let handle = self.handle.clone();

        tauri::async_runtime::spawn_blocking(move || {
            let (song, duration) = song_data;
            let duration_ms = duration
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            let state = handle.state::<Mutex<Option<DiscordRpcClient>>>();
            let mut lock = state.lock().unwrap();

            if lock.is_none() {
                match DiscordRpcClient::new(handle.clone()) {
                    Ok(client) => *lock = Some(client),
                    Err(e) => {
                        eprintln!("Failed to create Discord client: {e}");
                        return;
                    }
                }
            }

            if let Some(ref mut discord) = *lock {
                if !discord.is_connected() && discord.reconnect().is_err() {
                    return;
                }

                if let Err(e) = discord.update_song(&song, duration_ms, pos_ms, is_paused) {
                    eprintln!("Discord update failed, disconnecting: {e}");
                    discord.is_connected = false;
                }
            }
        });
    }

    fn reconnect_default_device(&mut self) -> Result<(), String> {
        println!("Attempting to reconnect to a default audio device...");

        let prev_is_paused = self.is_paused();
        let prev_volume = self.get_volume();
        let prev_pos = self.player.get_pos();
        let prev_song = self.current_song.clone();

        let new_sink = Self::create_sink()?;
        let new_player = Player::connect_new(new_sink.mixer());

        self.sink = new_sink;
        self.player = new_player;

        if let Some(song) = prev_song {
            if let Ok(file) = File::open(&song.path) {
                if let Ok(source) = Decoder::try_from(file) {
                    self.player.append(source);

                    if prev_pos.as_millis() > 0 {
                        let _ = self.player.try_seek(prev_pos);
                    }

                    self.set_volume(prev_volume);

                    if prev_is_paused {
                        self.player.pause();
                    } else {
                        self.player.play();
                    }
                }
            }
        }

        self.current_device_id = Self::get_default_device_id().ok();

        self.emit_state();
        Ok(())
    }

    pub fn set_queue(&mut self, songs: Vec<Song>) {
        println!("SETTING QUEUE: {songs:?}");
        self.queue = songs;
    }

    pub fn set_volume(&mut self, fraction: f32) {
        self.player.set_volume(fraction);
    }

    pub fn get_volume(&mut self) -> f32 {
        self.player.volume()
    }

    fn spawn_cover_listener(handle: &AppHandle) {
        use tauri::Listener;

        let listener_handle = handle.clone();

        handle.listen_any("cover-found", move |event| {
            let Ok(payload) = serde_json::from_str::<(i64, String)>(event.payload()) else {
                return;
            };
            let (song_id, url) = payload;

            let h = listener_handle.clone();

            tauri::async_runtime::spawn(async move {
                if let Some(state) = h.try_state::<Mutex<AudioPlayer>>() {
                    if let Ok(mut player) = state.lock() {
                        if let Some(ref mut song) = player.current_song {
                            if song.id == Some(song_id) {
                                song.external_cover_url = Some(url);
                                println!("Sync: Song cover updated in AudioPlayer state.");
                            }
                        }
                    }
                }
            });
        });
    }
}
