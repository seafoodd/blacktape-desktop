use rodio::{Decoder, MixerDeviceSink, Player, Source};
use souvlaki::MediaMetadata;
use std::{
    fs::{self, File},
    io::Write,
    sync::Mutex,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    audio::media_controls::MediaControls,
    discord_presence,
    types::{PlayerState, Song},
};

pub struct AudioPlayer {
    _stream: MixerDeviceSink, // must keep alive
    player: Player,
    duration: Option<Duration>,
    media_controls: MediaControls,
    current_song: Option<Song>,
    handle: AppHandle,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls, handle: AppHandle) -> Self {
        let stream =
            rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
        let player = rodio::Player::connect_new(&stream.mixer());

        Self {
            _stream: stream,
            player,
            duration: None,
            media_controls,
            current_song: None,
            handle,
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
        self.player.play();
        self.current_song = Some(song.clone());

        let uri = AudioPlayer::cover_file_uri(&song);
        println!("debug: {:#?}", uri);
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

    pub fn next(&mut self) {
        println!("NEXXXTTTT!!!! (unimplemented)")
    }

    pub fn previous(&mut self) {
        println!("PREVIOUUS!!!! (unimplemented)")
    }

    pub fn seek(&self, fraction: f32) {
        let Some(duration) = self.duration else {
            return;
        };
        let target = duration.mul_f32(fraction);
        println!("Seeking: {:?}", target);
        if let Err(e) = self.player.try_seek(target) {
            eprintln!("Seek failed: {:?}", e);
        }

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
            progress: self.position(),
        };
        println!(
            "emmited {}, {:#?}, {}\n {}, {}\n\n",
            state.current_song.clone().unwrap().title,
            state.current_song.clone().unwrap().duration,
            state.current_song.clone().unwrap().path,
            state.is_playing,
            state.progress
        );

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
        let discord = self
            .handle
            .state::<Mutex<discord_presence::DiscordRpcClient>>();
        let mut discord = discord.lock().unwrap();

        if let Some(duration) = self.duration {
            let pos_ms = self.player.get_pos().as_millis() as i64;
            let duration_ms = duration.as_millis() as i64;

            if let Err(e) = discord.update_timestamps(pos_ms, duration_ms) {
                eprintln!("Failed to update Discord timestamps: {}", e);
            }
        }
    }

    fn update_discord_song(&self) {
        let song_data = match &self.current_song {
            Some(song) => (song.clone(), self.duration),
            None => return,
        };

        let handle = self.handle.clone();

        tauri::async_runtime::spawn(async move {
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
