use rodio::{Decoder, MixerDeviceSink, Player, Source};
use std::{fs::File, time::Duration};

pub struct AudioPlayer {
    player: Player,
    _stream: MixerDeviceSink, // must keep alive
    duration: Option<Duration>,
}

impl AudioPlayer {
    pub fn new() -> Self {
        let handle =
            rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
        let player = rodio::Player::connect_new(&handle.mixer());

        Self {
            player,
            _stream: handle,
            duration: None,
        }
    }

    pub fn play(&mut self, path: String) {
        let file = File::open(&path).expect("failed to open file");
        let source = Decoder::try_from(file).expect("failed to decode audio");
        self.duration = source.total_duration();

        self.player.clear();
        self.player.append(source);
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn resume(&self) {
        self.player.play();
    }

    pub fn stop(&mut self) {
        self.player.stop();
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
}
