use crate::{
    audio::media_controls::MediaControls,
    discord_presence,
    types::{PlayerState, Song},
};
use rodio::{Decoder, MixerDeviceSink, Player, Source};
use souvlaki::MediaMetadata;
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

pub struct AudioPlayer {
    _stream: MixerDeviceSink,
    player: Player,
    duration: Option<Duration>,
    media_controls: MediaControls,
    current_song: Option<Song>,
    handle: AppHandle,
    queue: VecDeque<Song>,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls, handle: AppHandle) -> Self {
        let stream =
            rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
        let player = rodio::Player::connect_new(&stream.mixer());

        let handle_clone = handle.clone();
        tauri::async_runtime::spawn(async move {
            let mut sleep_time: Duration = Duration::ZERO;
            let mut is_next = false;

            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let state = handle_clone.state::<Mutex<AudioPlayer>>();

                let needs_emit = {
                    let mut player = state.lock().unwrap();

                    if player.is_paused() {
                        false
                    } else {
                        let current_pos = player.player.get_pos();
                        let mut advanced = false;

                        if let Some(duration) = player.duration {
                            if duration.saturating_sub(current_pos) <= Duration::from_millis(250) {
                                // println!("start seamless transition");
                                is_next = player.advance_to_next_in_queue();

                                player.buffer_next_silent();
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

                    let mut player = state.lock().unwrap();
                    if !is_next {
                        player.pause();
                        player.stop();
                    }

                    player.emit_state();
                    // println!("state emitted after delay");
                }
            }
        });

        Self {
            _stream: stream,
            player: player,
            duration: None,
            media_controls,
            current_song: None,
            handle,
            queue: VecDeque::new(),
        }
    }

    pub fn play(&mut self, song: Song) {
        println!("{}, {:#?}", song.title, song.duration);
        let path = &song.path;
        let file = File::open(&path).expect("failed to open file");
        let source = Decoder::try_from(file).expect("failed to decode audio");
        self.duration = source.total_duration();

        self.player.clear();
        self.player.append(source);

        self.clear_queue();
        // self.add_to_queue(Song {
        //     path: "Z:\\music\\glass beach\\plastic death\\glass beach - plastic death - 10 the CIA.mp3".to_string(),
        //     title: "the CIA".to_string(),
        //     artist: "glass beach".to_string(),
        //     album: "plastic death".to_string(),
        //     duration: Duration::new(282, 0),
        //     cover: None,
        // });
        self.player.play();
        self.current_song = Some(song.clone());

        let uri = AudioPlayer::cover_file_uri(&song);
        // println!("debug: {:#?}", uri);
        self.media_controls.update_metadata(MediaMetadata {
            title: Some(&song.title),
            artist: Some(&song.artist),
            album: Some(&song.album),
            duration: Some(song.duration),
            cover_url: uri.as_deref(),
        });
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
        let path = &song.path;
        let file = File::open(&path).expect("failed to open file");
        let source = Decoder::try_from(file).expect("failed to decode audio");
        self.player.append(source);
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
            self.update_discord_song();
            self.media_controls.update_metadata(MediaMetadata {
                title: Some(&next.title),
                artist: Some(&next.artist),
                album: Some(&next.album),
                duration: Some(next.duration),
                cover_url: None,
            });
            self.duration = Some(next.duration);
            self.current_song = Some(next);
            return true;
        }
        false
    }

    fn buffer_next_silent(&mut self) {
        if let Some(next) = self.queue.front() {
            if let Ok(file) = File::open(&next.path) {
                if let Ok(source) = Decoder::try_from(file) {
                    self.player.append(source);
                }
            }
        }
    }

    pub fn next(&mut self) {
        self.player.skip_one();
        self.advance_to_next_in_queue();
        self.buffer_next_silent();
        self.emit_state();
    }

    pub fn previous(&mut self) {
        println!("PREVIOUUS!!!! (unimplemented)")
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
        self.update_discord_timestamp()
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

    fn cover_file_uri(song: &Song) -> Option<String> {
        song.cover.as_ref().map(|cover_bytes| {
            let mut temp_path = std::env::temp_dir();

            temp_path.push("blacktape");
            temp_path.push(format!("current_song_cover.jpg"));

            if let Some(parent) = temp_path.parent() {
                fs::create_dir_all(parent).expect("Failed to create temp directory");
            }
            let mut f = File::create(&temp_path).expect("Failed to create temp cover file");
            f.write_all(cover_bytes)
                .expect("Failed to write temp cover file");

            let path_str = temp_path.to_string_lossy();

            #[cfg(target_os = "windows")]
            let cover_path = format!("file://{}", path_str.replace('/', "\\"));
            #[cfg(not(target_os = "windows"))]
            let cover_path = format!("file://{}", path_str);

            cover_path
        })
    }

    fn update_discord_timestamp(&self) {
        if let Ok(mut discord) = self
            .handle
            .state::<Mutex<discord_presence::DiscordRpcClient>>()
            .try_lock()
        {
            if let Some(duration) = self.duration {
                let pos_ms = self.player.get_pos().as_millis() as i64;
                let duration_ms = duration.as_millis() as i64;

                if let Err(e) = discord.update_timestamps(pos_ms, duration_ms) {
                    eprintln!("Failed to update Discord timestamps: {}", e);
                }
            }
        }
    }

    fn update_discord_song(&self) {
        let song_data = match &self.current_song {
            Some(song) => (song.clone(), self.duration),
            None => return,
        };

        let handle = self.handle.clone();

        let _ = tauri::async_runtime::spawn(async move {
            let (song, duration) = song_data;
            let duration_ms = duration.map(|d| d.as_millis() as i64).unwrap_or(0);

            let _ = tauri::async_runtime::spawn_blocking(move || {
                let discord_guard = handle.state::<Mutex<discord_presence::DiscordRpcClient>>();
                let mut discord = discord_guard.lock().unwrap();
                discord.update_song(&song, duration_ms)
            })
            .await
            .map_err(|e| eprintln!("Discord RPC task failed: {}", e));
        });
    }
}
