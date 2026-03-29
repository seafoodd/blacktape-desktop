use rodio::{Decoder, MixerDeviceSink, Player, Source};
use souvlaki::MediaMetadata;
use std::{
    fs::{self, File},
    io::Write,
    time::Duration,
};

use crate::{audio::media_controls::MediaControls, types::Song};

pub struct AudioPlayer {
    _stream: MixerDeviceSink, // must keep alive
    player: Player,
    duration: Option<Duration>,
    media_controls: MediaControls,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls) -> Self {
        let handle =
            rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
        let player = rodio::Player::connect_new(&handle.mixer());

        Self {
            _stream: handle,
            player,
            duration: None,
            media_controls,
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

        let uri = AudioPlayer::cover_file_uri(&song);
        println!("debug: {:#?}", uri);
        self.media_controls.update_metadata(MediaMetadata {
            title: Some(&song.title),
            artist: Some(&song.artist),
            album: Some(&song.album),
            duration: Some(song.duration),
            cover_url: uri.as_deref(),
        });
        self.media_controls.play();
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.media_controls.pause();
    }

    pub fn resume(&mut self) {
        self.player.play();
        self.media_controls.play();
    }

    pub fn stop(&mut self) {
        self.player.stop();
        self.media_controls.stop();
        self.duration = None;
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
}
