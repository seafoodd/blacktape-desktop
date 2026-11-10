//! Audio output of the player: the system's default device, or a silent stand-in when there is none.
//!
//! Without a device (a machine with no sound card, a remote desktop session without audio) the player must still
//! work: the queue, the position and the state the UI shows all depend on the mixer consuming samples. The
//! [`AudioOutput::Null`] variant feeds the mixer into a thread that discards samples at real-time speed, so playback
//! advances exactly as it would on a real device. The device watcher switches to a real device once one appears.

use rodio::mixer::{Mixer, MixerSource};
use rodio::{MixerDeviceSink, Source};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Format of the silent output (any valid format works: the mixer converts every source to it).
const NULL_CHANNELS: u16 = 2;
const NULL_SAMPLE_RATE: u32 = 44_100;
/// How much audio the silent output consumes per wake-up.
const NULL_TICK: Duration = Duration::from_millis(10);

pub enum AudioOutput {
    Device(MixerDeviceSink),
    Null(NullOutput),
}

impl AudioOutput {
    /// Opens the system's default output device.
    pub fn open_default() -> Result<Self, String> {
        rodio::DeviceSinkBuilder::from_default_device()
            .map_err(|e| format!("no default audio device: {e}"))?
            .open_stream()
            .map(AudioOutput::Device)
            .map_err(|e| format!("cannot open the default audio device: {e}"))
    }

    /// The default device, or the silent output when it cannot be opened (the reason is logged).
    pub fn open_default_or_null() -> Self {
        Self::open_default().unwrap_or_else(|e| {
            eprintln!("[audio] {e}; playing to a silent output until a device is available");
            AudioOutput::Null(NullOutput::start())
        })
    }

    pub fn mixer(&self) -> &Mixer {
        match self {
            AudioOutput::Device(sink) => sink.mixer(),
            AudioOutput::Null(null) => &null.mixer,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, AudioOutput::Null(_))
    }
}

/// A mixer whose output is consumed and thrown away at real-time speed by a background thread.
pub struct NullOutput {
    mixer: Mixer,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NullOutput {
    pub fn start() -> Self {
        let channels = NonZero::new(NULL_CHANNELS).expect("non-zero channel count");
        let sample_rate = NonZero::new(NULL_SAMPLE_RATE).expect("non-zero sample rate");
        let (mixer, source) = rodio::mixer::mixer(channels, sample_rate);
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let thread = std::thread::Builder::new()
            .name("audio-null-output".into())
            .spawn(move || drain_in_real_time(source, &flag))
            .map_err(|e| eprintln!("[audio] cannot start the silent output thread: {e}"))
            .ok();
        Self { mixer, running, thread }
    }
}

impl Drop for NullOutput {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Number of samples that make up `elapsed` of audio in the given format.
pub fn samples_for(elapsed: Duration, sample_rate: u32, channels: u16) -> u64 {
    let per_second = u128::from(sample_rate) * u128::from(channels);
    u64::try_from(elapsed.as_nanos() * per_second / 1_000_000_000).unwrap_or(u64::MAX)
}

fn drain_in_real_time(mut source: MixerSource, running: &AtomicBool) {
    let started = Instant::now();
    let mut consumed: u64 = 0;
    while running.load(Ordering::Acquire) {
        std::thread::sleep(NULL_TICK);
        let due = samples_for(started.elapsed(), source.sample_rate().get(), source.channels().get());
        while consumed < due {
            // the mixer source never ends on its own; `None` would only mean it was torn down
            if source.next().is_none() {
                return;
            }
            consumed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_for_one_second_is_rate_times_channels() {
        assert_eq!(samples_for(Duration::from_secs(1), 44_100, 2), 88_200);
    }

    #[test]
    fn samples_for_partial_durations_round_down() {
        assert_eq!(samples_for(Duration::from_millis(10), 44_100, 2), 882);
        assert_eq!(samples_for(Duration::from_nanos(1), 44_100, 2), 0);
        assert_eq!(samples_for(Duration::ZERO, 48_000, 1), 0);
    }

    #[test]
    fn null_output_plays_a_player_to_the_end_without_a_device() {
        let output = AudioOutput::Null(NullOutput::start());
        let player = rodio::Player::connect_new(output.mixer());
        let tone = rodio::source::SineWave::new(440.0).take_duration(Duration::from_millis(150));
        player.append(tone);
        assert_eq!(player.len(), 1);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !player.empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(player.empty(), "the silent output consumes the source in real time");
    }

    #[test]
    fn null_output_clear_does_not_block() {
        let output = AudioOutput::Null(NullOutput::start());
        let player = rodio::Player::connect_new(output.mixer());
        player.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_secs(30)));
        let started = Instant::now();
        player.clear();
        assert!(started.elapsed() < Duration::from_secs(2), "clear waits for the mixer, which keeps running");
        assert!(output.is_null());
    }
}
