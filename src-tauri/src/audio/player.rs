use crate::discord_presence::DiscordRpcClient;
use crate::{
    audio::media_controls::MediaControls,
    types::{PlayerState, Song},
};
use rodio::cpal::{
    self,
    traits::{DeviceTrait, HostTrait},
    DeviceId, DeviceIdError,
};
use rodio::{Decoder, MixerDeviceSink, Player, Source};
use souvlaki::MediaMetadata;
use std::time::Instant;
use std::{collections::VecDeque, fs::File, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

enum RepeatMode {
    Off,
    Track,
    Queue,
}

pub struct AudioPlayer {
    _sink: MixerDeviceSink,
    player: Player,
    duration: Option<Duration>,
    media_controls: MediaControls,
    current_song: Option<Song>,
    handle: AppHandle,
    history: VecDeque<Song>,
    queue: VecDeque<Song>,
    current_device_id: Option<DeviceId>,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls, handle: AppHandle) -> Self {
        let sink = Self::create_sink().expect("Failed to open default audio stream");
        let player = Player::connect_new(&sink.mixer());

        Self::spawn_device_watcher(handle.clone());
        Self::spawn_transition_watcher(handle.clone());

        let initial_device_id = Self::get_default_device_id().ok();

        Self {
            _sink: sink,
            player,
            duration: None,
            media_controls,
            current_song: None,
            handle,
            history: VecDeque::new(),
            queue: VecDeque::new(),
            current_device_id: initial_device_id,
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
                    let mut player = state.lock().unwrap_or_else(|e| e.into_inner());

                    if player.is_paused() {
                        false
                    } else {
                        let current_pos = player.player.get_pos();
                        let mut advanced = false;

                        if let Some(duration) = player.duration {
                            if duration.saturating_sub(current_pos) <= Duration::from_millis(250) {
                                // println!("start seamless transition");
                                player.buffer_next_silent();
                                is_next = player.advance_to_next_in_queue();
                                // println!("seamless transition IS NEXT: {}", is_next);

                                sleep_time = duration.saturating_sub(current_pos);
                                advanced = true;
                            }
                        }
                        advanced
                    }
                };

                if needs_emit {
                    // println!("sleeping for {:#?}", sleep_time);
                    tokio::time::sleep(sleep_time).await;

                    let mut player = state.lock().unwrap_or_else(|e| e.into_inner());

                    if !is_next {
                        player.pause();
                        player.stop();
                    }

                    player.emit_state();
                    player.update_discord_song();
                    player.update_current_metadata();
                    // println!("state emitted after delay");
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
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Could not open file {}: {}", path, e);
                return;
            }
        };
        let source = match Decoder::try_from(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Failed to decode audio for {}: {}", path, e);
                return;
            }
        };
        self.duration = source.total_duration();

        self.player.clear();
        self.player.append(source);
        // self.buffer_next_silent();

        // self.clear_queue();

        // let test_songs = [
        //     "Z:\\music\\glass beach\\plastic death\\glass beach - plastic death - 10 the CIA.mp3",
        //     "Z:\\music\\Heaven Pierce Her\\ULTRAKILL - FRAUD\\Heaven Pierce Her - ULTRAKILL- FRAUD - 07 The Shattering Circle, or- A Charade of Shadeless Ones and Zeroes Rearranged ad Nihilum.mp3",
        //     "Z:\\music\\Bull of Heaven\\Superstring Theory Verified\\Bull of Heaven - 111- Superstring Theory Verified - 01 Superstring Theory Verified.mp3",
        // ];

        // for p in test_songs {
        //     if let Some(s) = get_song_from_path(p) {
        //         self.add_to_queue(s);
        //     }
        // }

        self.player.play();
        self.current_song = Some(song.clone());

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
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.media_controls
            .stop()
            .expect("Failed to stop media controls state");
        self.duration = None;
        self.emit_state();
    }

    pub fn toggle(&mut self) {
        if self.is_paused() {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn add_to_queue(&mut self, song: Song) {
        self.queue.push_back(song);

        // TODO: send queue to the client maybe
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    pub fn get_queue(&self) -> Vec<Song> {
        self.queue.iter().cloned().collect()
    }

    /// Advances the player to the next song in the queue.
    ///
    /// Returns `true` if the player advanced, or `false` if the queue was empty.
    fn advance_to_next_in_queue(&mut self) -> bool {
        if let Some(next) = self.queue.pop_front() {
            println!("advance_to_next_in_queue: {:?}", next.id);
            let duration = Duration::from_millis(next.duration_ms);

            if let Some(finished) = self.current_song.take() {
                self.history.push_back(finished);
            }

            self.current_song = Some(next.clone());
            self.duration = Some(duration);

            // self.update_discord_song();
            // self.update_current_metadata();

            return true;
        }
        false
    }

    fn update_current_metadata(&mut self) {
        let Some(song) = &self.current_song else {
            return;
        };

        let formatted_uri: String;

        let uri_ref = if let Some(path) = &song.cover_url {
            formatted_uri = Self::format_cover_path(path.clone());
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

    fn buffer_next_silent(&mut self) {
        let now = Instant::now();
        if let Some(next) = self.queue.front() {
            println!("Silent buffer: {}, {:?}", next.title, next.id);
            if let Ok(file) = File::open(&next.path) {
                if let Ok(source) = Decoder::try_from(file) {
                    self.player.append(source);
                }
            }
        }
        println!("BUFFERING TOOK: {:#?}", now.elapsed())
    }

    pub fn next(&mut self) {
        self.player.skip_one();
        self.buffer_next_silent();
        if !self.advance_to_next_in_queue() {
            self.pause();
            self.stop();
        }
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song();
    }

    pub fn previous(&mut self) {
        if self.player.get_pos() > Duration::from_secs(5) {
            if let Some(current) = self.current_song.clone() {
                self.play(current);
                return;
            }
        }

        if let Some(prev_song) = self.history.pop_back() {
            if let Some(current) = self.current_song.take() {
                self.queue.push_front(current);
            }

            self.play(prev_song);
        } else {
            if let Some(current) = self.current_song.clone() {
                self.play(current);
            }
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
            return self.next();
        }

        if let Err(e) = self.player.try_seek(target) {
            eprintln!("Seek failed: {:?}", e);
        }
        println!(
            "seeking: {:?}, REMAININGGGGGGG {:?}, duration: {:?}",
            target, remaining, duration
        );
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
            // progress: 0.0,
            progress: self.position(),
        };

        if let Some(ref song) = state.current_song {
            println!(
                "emitted {}, progress: {}, is_playing: {}",
                song.title, state.progress, state.is_playing
            );
            // println!("queue: {:#?}", self.get_queue());
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

    fn format_cover_path(path: String) -> String {
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

        let pos_ms = self.player.get_pos().as_millis() as i64;
        let handle = self.handle.clone();
        let handle_clone = self.handle.clone();

        tauri::async_runtime::spawn(async move {
            let (song, duration) = song_data;
            let duration_ms = duration.map(|d| d.as_millis() as i64).unwrap_or(0);

            let _ = tauri::async_runtime::spawn_blocking(move || {
                let state = handle.state::<Mutex<Option<DiscordRpcClient>>>();
                let mut lock = state.lock().unwrap();

                if lock.is_none() {
                    match DiscordRpcClient::new(handle_clone) {
                        Ok(client) => *lock = Some(client),
                        Err(e) => {
                            eprintln!("Failed to create Discord client: {}", e);
                            return;
                        }
                    }
                }

                if let Some(ref mut discord) = *lock {
                    if !discord.is_connected() {
                        if let Err(_) = discord.reconnect() {
                            return;
                        }
                    }

                    if let Err(e) = discord.update_song(&song, duration_ms, pos_ms) {
                        eprintln!("Discord update failed, disconnecting: {}", e);
                        discord.is_connected = false;
                    }
                }
            });
        });
    }

    fn reconnect_default_device(&mut self) -> Result<(), String> {
        println!("Attempting to reconnect to a default audio device...");

        let was_playing = !self.is_paused();
        let current_pos = self.player.get_pos();
        let current_song = self.current_song.clone();

        let new_sink = Self::create_sink()?;
        let new_player = Player::connect_new(&new_sink.mixer());

        self._sink = new_sink;
        self.player = new_player;

        if let Some(song) = current_song {
            if let Ok(file) = File::open(&song.path) {
                if let Ok(source) = Decoder::try_from(file) {
                    self.player.append(source);

                    if current_pos.as_millis() > 0 {
                        let _ = self.player.try_seek(current_pos);
                    }

                    if !was_playing {
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

    pub fn set_history(&mut self, songs: VecDeque<Song>) {
        println!("SETTING HISTORY: {:?}", songs);
        self.history = songs;
    }
    pub fn set_queue(&mut self, songs: VecDeque<Song>) {
        println!("SETTING QUEUE: {:?}", songs);
        self.queue = songs;
    }
}
