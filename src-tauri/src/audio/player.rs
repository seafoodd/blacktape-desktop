use crate::discord_presence::DiscordRpcClient;
use crate::audio::queue_ops;
use crate::download::opus_source::OpusSource;
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
use std::path::Path;
use std::time::Instant;
use std::{fs::File, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

/// A song of the queue that is streamed from its platform instead of played from a file.
pub fn is_stream_song(song: &Song) -> bool {
    song.path.is_empty() && song.source_url.as_deref().is_some_and(|u| !u.is_empty())
}

/// Index in the original queue of the song at `cursor` in the play order.
pub fn source_index_at(play_order: Option<&[usize]>, cursor: Option<usize>) -> Option<usize> {
    play_order?.get(cursor?).copied()
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    Track,
    Queue,
}

pub type BoxedSource = Box<dyn Source<Item = f32> + Send>;

pub struct AudioPlayer {
    sink: MixerDeviceSink,
    player: Player,
    duration: Option<Duration>,
    seek_offset: Duration,
    media_controls: MediaControls,
    current_song: Option<Song>,
    handle: AppHandle,
    queue: Vec<Song>,
    current_device_id: Option<DeviceId>,
    shuffle_mode: bool,
    repeat_mode: RepeatMode,
    play_order: Option<Vec<usize>>,
    cursor: Option<usize>,
    /// Url of the stream song that is being opened right now (nothing plays meanwhile).
    stream_loading: Option<String>,
    /// Bumped for every stream load; a finished load only applies when it is still the newest.
    stream_generation: u64,
    /// The next song of the queue, already opened (stream songs only): url, source, duration.
    prepared: Option<(String, BoxedSource, Option<Duration>)>,
    /// Url of the stream song being prepared in the background.
    preparing: Option<String>,
    /// Duration of a prepared stream that was appended for a gapless transition.
    appended_duration: Option<Duration>,
}

/// Playback position saved while the current song's file is released for a tag edit.
pub struct EditSuspend {
    pos: Duration,
    paused: bool,
}

/// Copies user-editable metadata from `updated` into `target` when both are the
/// same song; everything else (path, duration, playback fields) is kept.
pub fn apply_song_edit(target: &mut Song, updated: &Song) -> bool {
    if target.id.is_none() || target.id != updated.id {
        return false;
    }
    target.title = updated.title.clone();
    target.artists = updated.artists.clone();
    target.album = updated.album.clone();
    target.album_artist = updated.album_artist.clone();
    target.track_number = updated.track_number;
    target.canonical_track_slug = updated.canonical_track_slug.clone();
    target.canonical_album_slug = updated.canonical_album_slug.clone();
    true
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
            seek_offset: Duration::ZERO,
            media_controls,
            current_song: None,
            handle,
            queue: Vec::new(),
            current_device_id: initial_device_id,
            shuffle_mode: false,
            repeat_mode: RepeatMode::Off,
            play_order: None,
            cursor: None,
            stream_loading: None,
            stream_generation: 0,
            prepared: None,
            preparing: None,
            appended_duration: None,
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

            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let state = handle.state::<Mutex<AudioPlayer>>();

                let mut pending_transition: Option<(usize, Song, bool)> = None;

                let needs_emit = {
                    let mut player = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);

                    if player.is_paused() || player.stream_loading.is_some() {
                        false
                    } else {
                        let current_pos = player.seek_offset + player.player.get_pos();
                        let mut advanced = false;

                        let stream_ran_out = player.player.empty()
                            && player.current_song.as_ref().is_some_and(is_stream_song);
                        if stream_ran_out {
                            // A stream ended before its announced duration (or nothing was buffered).
                            if let Some(next_cursor) = player.get_next_cursor() {
                                if let Some(next_song) = player.song_at(next_cursor) {
                                    pending_transition = Some((next_cursor, next_song, false));
                                }
                            }
                            sleep_time = Duration::ZERO;
                            advanced = true;
                        } else if let Some(duration) = player.duration {
                            if duration.saturating_sub(current_pos) <= Duration::from_millis(250) {
                                println!("start seamless transition");

                                if let Some(next_cursor) = player.get_next_cursor() {
                                    if let Some((next_song, appended)) =
                                        player.buffer_next_in_queue(next_cursor)
                                    {
                                        pending_transition = Some((next_cursor, next_song, appended));
                                        sleep_time = duration.saturating_sub(current_pos);
                                        advanced = true;
                                    }
                                } else {
                                    player.stop();
                                    pending_transition = None;
                                    sleep_time = Duration::ZERO;
                                    advanced = true;
                                }
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

                    match pending_transition.take() {
                        Some((next_cursor, next_song, true)) => {
                            // Seamless: the next source was already appended.
                            player.cursor = Some(next_cursor);
                            player.current_song = Some(next_song.clone());
                            player.duration = player
                                .appended_duration
                                .take()
                                .or(Some(Duration::from_millis(next_song.duration_ms)));
                            player.seek_offset = Duration::ZERO;
                            player.adopt_stream_duration();
                            player.prepare_next();
                        }
                        Some((next_cursor, next_song, false)) => {
                            // Nothing could be appended (stream not ready yet): load it now.
                            player.cursor = Some(next_cursor);
                            player.play(next_song);
                            continue;
                        }
                        None => player.stop(),
                    }

                    player.emit_state();
                    player.update_discord_song(Some(0));
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
        if is_stream_song(&song) {
            return self.play_stream_song(song);
        }
        self.stream_loading = None;
        let source = match Self::decode_source(&song.path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error playing {}: {e}", song.path);
                return;
            }
        };

        self.duration = source
            .total_duration()
            .or_else(|| Some(Duration::from_millis(song.duration_ms)));

        self.seek_offset = Duration::ZERO; // RESET OFFSET ON PLAY

        self.player.clear();
        self.player.append(source);

        self.player.play();
        self.current_song = Some(song);

        self.update_current_metadata();
        self.media_controls
            .play()
            .expect("Failed to resume media controls state");
        self.emit_state();
        self.update_discord_song(Some(0));
        self.prepare_next();
    }

    fn decode_source(path_str: &str) -> Result<BoxedSource, Box<dyn std::error::Error>> {
        let path = Path::new(path_str);

        // 1. If it's an .opus file, use your custom OpusSource
        if path.extension().and_then(|ext| ext.to_str()) == Some("opus") {
            let opus_source = OpusSource::open(path)?;
            return Ok(Box::new(opus_source));
        }

        // 2. Fall back to Rodio's generic decoder for MP3, WAV, FLAC, etc.
        let file = File::open(path)?;
        let decoder = Decoder::try_from(file)?;

        // Box and return
        Ok(Box::new(decoder))
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.media_controls
            .pause()
            .expect("Failed to pause media controls");
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song(None);
    }

    pub fn resume(&mut self) {
        if self.player.len() < 1 {
            if self.stream_loading.is_some() {
                return;
            }
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
        self.update_discord_song(None);
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
    }

    /// Id of the loaded (current) song, if any.
    pub fn current_song_id(&self) -> Option<i64> {
        self.current_song.as_ref().and_then(|s| s.id)
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

        self.emit_state();
    }

    pub fn add_next_to_queue(&mut self, song: Song) {
        self.queue.push(song);
        let new_idx = self.queue.len() - 1;

        match (&mut self.play_order, self.cursor) {
            (Some(order), Some(cursor_pos)) => {
                let insert_pos = (cursor_pos + 1).min(order.len());
                order.insert(insert_pos, new_idx);
            }
            (Some(order), None) => {
                order.push(new_idx);
                self.cursor = Some(0);
            }
            (None, _) => {
                self.play_order = Some(vec![new_idx]);
                self.cursor = Some(0);
            }
        }

        self.emit_state();
    }

    /// Removes the songs at `positions` (positions in the play order). Removing the
    /// current song moves on to the song after it, or stops if there is none.
    pub fn remove_from_queue(&mut self, positions: &[usize]) {
        let Some(order) = self.play_order.clone() else {
            return;
        };
        let removal = queue_ops::remove_positions(&order, self.cursor, positions);
        if removal.keep.len() == order.len() {
            return; // nothing valid to remove
        }

        let new_queue: Vec<Song> = removal
            .keep
            .iter()
            .filter_map(|&i| self.queue.get(i).cloned())
            .collect();
        self.queue = new_queue;
        self.play_order = Some(removal.order);
        self.cursor = removal.cursor;

        if removal.removed_current {
            match self.get_song_at_cursor() {
                Some(next) => {
                    self.play(next);
                    return;
                }
                None => {
                    self.stop();
                    self.current_song = None;
                    self.cursor = None;
                }
            }
        }
        self.emit_state();
    }

    /// Moves the songs at `positions` as a block to before `insert_before` (an index
    /// in the current play order). The current song keeps playing.
    pub fn move_in_queue(&mut self, positions: &[usize], insert_before: usize) {
        let Some(order) = self.play_order.as_ref() else {
            return;
        };
        let (new_order, new_cursor) =
            queue_ops::move_positions(order, self.cursor, positions, insert_before);
        self.play_order = Some(new_order);
        self.cursor = new_cursor;
        self.emit_state();
    }

    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    pub fn get_queue(&self) -> Vec<Song> {
        self.queue.clone()
    }

    pub fn get_ordered_queue(&self) -> Vec<Song> {
        match &self.play_order {
            Some(order) => order
                .iter()
                .filter_map(|&idx| self.queue.get(idx).cloned())
                .collect(),
            None => self.queue.clone(),
        }
    }

    /// Advances the player to the next song in the queue.
    ///
    /// Returns `true` if the player advanced, or `false` if the queue was empty.
    fn advance_to_next_in_queue(&mut self, next_cursor: usize) -> bool {
        self.cursor = Some(next_cursor);

        if let Some(next_song) = self.get_song_at_cursor() {
            if is_stream_song(&next_song) {
                self.play(next_song);
                return true;
            }
            self.buffer_next_silent(&next_song);
            self.current_song = Some(next_song.clone());
            self.duration = Some(Duration::from_millis(next_song.duration_ms));
            self.seek_offset = Duration::ZERO;
            self.prepare_next();
            true
        } else {
            false
        }
    }

    /// Appends the song at `next_cursor` for a gapless transition. Returns the song and whether audio was
    /// appended (a stream song that is not prepared yet is not).
    fn buffer_next_in_queue(&mut self, next_cursor: usize) -> Option<(Song, bool)> {
        let next_song = self.song_at(next_cursor)?;
        let appended = self.buffer_next_silent(&next_song);
        Some((next_song, appended))
    }

    /// Song at play-order position `cursor`.
    fn song_at(&self, cursor: usize) -> Option<Song> {
        let idx = *self.play_order.as_ref()?.get(cursor)?;
        self.queue.get(idx).cloned()
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

    pub fn play_song_in_queue(&mut self, index: i64) {
        let Ok(target_cursor) = usize::try_from(index) else {
            return;
        };

        let song_to_play = match &self.play_order {
            Some(order) => order
                .get(target_cursor)
                .and_then(|&song_idx| self.queue.get(song_idx).cloned()),
            None => self.queue.get(target_cursor).cloned(),
        };

        if let Some(song) = song_to_play {
            self.cursor = Some(target_cursor);
            self.play(song);
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
        }
    }

    fn update_current_metadata(&mut self) {
        let Some(song) = &self.current_song else {
            return;
        };

        let formatted_uri: String;

        let uri_ref = if let Some(path) = &song.cover_url {
            formatted_uri = if path.starts_with("http://") || path.starts_with("https://") {
                path.clone()
            } else {
                Self::format_cover_path(path)
            };
            Some(formatted_uri.as_str())
        } else {
            None
        };

        let display_artists = song.artists.join(", ");

        self.media_controls.update_metadata(MediaMetadata {
            title: Some(&song.title),
            artist: Some(&display_artists),
            album: Some(&song.album),
            duration: Some(Duration::from_millis(song.duration_ms)),
            cover_url: uri_ref,
        });
    }

    fn buffer_next_silent(&mut self, song: &Song) -> bool {
        if is_stream_song(song) {
            let url = song.source_url.clone().unwrap_or_default();
            return match self.take_prepared(&url) {
                Some((source, duration)) => {
                    self.appended_duration = duration;
                    self.player.append(source);
                    true
                }
                None => false,
            };
        }
        let now = Instant::now();
        println!("Silent buffer: {}, {:?}", song.title, song.id);
        let appended = match Self::decode_source(&song.path) {
            Ok(source) => {
                self.player.append(source);
                true
            }
            Err(_) => false,
        };
        println!("BUFFERING TOOK: {:#?}", now.elapsed());
        appended
    }

    pub fn next(&mut self) {
        self.player.skip_one();
        self.player.play();

        let Some(order) = &self.play_order else {
            return;
        };
        let Some(current_cursor) = self.cursor else {
            return;
        };

        let mut next_cursor = current_cursor + 1;

        if next_cursor >= order.len() {
            if self.repeat_mode != RepeatMode::Off {
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
        self.update_discord_song(Some(0));
    }

    pub fn previous(&mut self) {
        let absolute_pos = self.seek_offset + self.player.get_pos();
        if absolute_pos > Duration::from_secs(5) {
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
        println!("[SEEK LOG] Triggered seek with fraction: {fraction}");

        let duration = self.duration.or_else(|| {
            self.current_song
                .as_ref()
                .map(|s| Duration::from_millis(s.duration_ms))
        });

        let Some(duration) = duration else {
            eprintln!("[SEEK LOG] Error: Duration is None and no current song available.");
            return;
        };

        self.duration = Some(duration);
        let target = duration.mul_f32(fraction);
        let remaining = duration.saturating_sub(target);

        println!(
            "[SEEK LOG] Duration: {duration:?}, Target: {target:?}, Remaining: {remaining:?}, Player Queue Len: {}",
            self.player.len()
        );

        if self.player.len() < 1 && self.stream_loading.is_some() {
            return;
        }
        if self.player.len() < 1 {
            println!("[SEEK LOG] Queue empty. Reloading current song...");
            if let Some(song) = self.current_song.clone() {
                self.play(song);
                return;
            }
        }

        if remaining < Duration::from_secs(1) {
            println!("[SEEK LOG] Near end of track (<1s remaining). Skipping to next track.");
            let Some(next_cursor) = self.get_next_cursor() else {
                return self.next();
            };

            self.player.skip_one();
            self.advance_to_next_in_queue(next_cursor);
            self.emit_state();
            self.update_current_metadata();
            return;
        }

        println!("[SEEK LOG] Attempting rodio player.try_seek({target:?})...");
        match self.player.try_seek(target) {
            Ok(()) => {
                println!("[SEEK LOG] try_seek succeeded natively.");
                self.seek_offset = Duration::ZERO;
            }
            Err(e) => {
                eprintln!("[SEEK LOG] Error: try_seek failed natively: {e:?}");
            }
        }

        let raw_pos = self.player.get_pos();
        let calculated_progress = (raw_pos.as_secs_f32() / duration.as_secs_f32()).min(1.0);

        println!(
            "[SEEK LOG] Position state after seek -> Raw rodio pos: {raw_pos:?}, Calculated fraction: {calculated_progress}"
        );

        let target_ms = i64::try_from(target.as_millis()).unwrap_or(0);
        self.update_discord_song(Some(target_ms));
        self.update_current_metadata();
        self.emit_state();
    }
    pub fn position(&self) -> f32 {
        if let Some(duration) = self.duration {
            let actual_pos = self.seek_offset + self.player.get_pos();
            return (actual_pos.as_secs_f32() / duration.as_secs_f32()).min(1.0);
        }
        0.0
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// Decodes an in-memory copy of an audio file (same format handling as `decode_source`).
    fn decode_source_from_memory(
        path_str: &str,
        bytes: Vec<u8>,
    ) -> Result<BoxedSource, Box<dyn std::error::Error>> {
        let path = Path::new(path_str);
        let len = bytes.len() as u64;
        let cursor = std::io::Cursor::new(bytes);

        if path.extension().and_then(|ext| ext.to_str()) == Some("opus") {
            return Ok(Box::new(OpusSource::from_reader(cursor, path.to_path_buf())?));
        }

        let decoder = Decoder::builder()
            .with_data(cursor)
            .with_byte_len(len)
            .with_seekable(true)
            .build()?;
        Ok(Box::new(decoder))
    }

    /// If `id` is the current song, switches playback to an in-memory copy of its
    /// file at (almost) the same position, so the file on disk is no longer open and
    /// can be rewritten with no audible gap. Does nothing for other songs. On error
    /// playback is left untouched.
    pub fn move_song_to_memory(&mut self, id: i64) -> Result<(), String> {
        let Some(song) = self
            .current_song
            .clone()
            .filter(|s| s.id == Some(id))
        else {
            return Ok(());
        };

        let bytes = std::fs::read(&song.path).map_err(|e| e.to_string())?;
        let mut source =
            Self::decode_source_from_memory(&song.path, bytes).map_err(|e| e.to_string())?;

        let now_pos = |p: &Self| p.seek_offset + p.player.get_pos();

        // The first seek only measures how long seeking takes, the second one lands
        // on where playback will be by the time the swap happens.
        let t0 = Instant::now();
        source
            .try_seek(now_pos(self))
            .map_err(|e| format!("seek failed: {e:?}"))?;
        let target = now_pos(self) + t0.elapsed();
        source
            .try_seek(target)
            .map_err(|e| format!("seek failed: {e:?}"))?;

        let paused = self.is_paused();
        self.seek_offset = target;
        if self.player.len() <= 1 {
            // append + skip_one hands over inside the mixer without clearing it
            self.player.append(source);
            self.player.skip_one();
        } else {
            // a gapless next track is already queued; it gets buffered again later
            self.player.clear();
            self.player.append(source);
        }
        if paused {
            self.player.pause();
        } else {
            self.player.play();
        }
        Ok(())
    }

    /// Fallback: if `id` is the current song, drops its open decoder (so the file can be
    /// rewritten safely) and returns the position to resume from.
    pub fn release_song_file(&mut self, id: i64) -> Option<EditSuspend> {
        if self.current_song.as_ref().and_then(|s| s.id) != Some(id) {
            return None;
        }
        let pos = self.seek_offset + self.player.get_pos();
        let paused = self.is_paused();
        self.player.clear();
        Some(EditSuspend { pos, paused })
    }

    /// Re-opens the current song's file after an edit and continues from where it was.
    pub fn restore_song_file(&mut self, suspended: EditSuspend) {
        let Some(song) = self.current_song.clone() else {
            return;
        };
        match Self::decode_source(&song.path) {
            Ok(mut source) => {
                if suspended.pos > Duration::ZERO {
                    let _ = source.try_seek(suspended.pos);
                }
                self.seek_offset = suspended.pos;
                self.player.append(source);
                if suspended.paused {
                    self.player.pause();
                } else {
                    self.player.play();
                }
            }
            Err(e) => eprintln!("Failed to reopen {} after tag edit: {e}", song.path),
        }
    }

    /// Applies edited metadata to the playing song and every queued copy of it,
    /// then pushes the new state to the UI and OS media controls.
    pub fn refresh_song_metadata(&mut self, updated: &Song) {
        if let Some(current) = self.current_song.as_mut() {
            apply_song_edit(current, updated);
        }
        for queued in self.queue.iter_mut() {
            apply_song_edit(queued, updated);
        }

        if self
            .current_song
            .as_ref()
            .is_some_and(|s| s.id == updated.id)
        {
            self.update_current_metadata();
            self.update_discord_song(None);
        }
        self.emit_state();
    }

    pub fn emit_state(&self) {
        let ordered_queue = self.get_ordered_queue();

        let state = PlayerState {
            current_song: self.current_song.clone(),
            is_playing: !self.is_paused(),
            progress: self.position(),
            volume: self.player.volume(),
            shuffle_mode: self.shuffle_mode,
            repeat_mode: self.repeat_mode,
            queue: ordered_queue,
            queue_cursor: self.cursor,
            source_index: source_index_at(self.play_order.as_deref(), self.cursor),
            streaming: self.current_song.as_ref().is_some_and(is_stream_song),
            stream_loading: self.stream_loading.is_some(),
        };

        if let Some(ref song) = state.current_song {
            println!(
                "emitted {}, progress: {}, is_playing: {}",
                song.title, state.progress, state.is_playing
            );
        }

        self.handle.emit("player-state", state).ok();
    }

    fn format_cover_path(path: &str) -> String {
        #[cfg(target_os = "windows")]
        let cover_path = format!("file://{}", path.replace('/', "\\"));
        #[cfg(not(target_os = "windows"))]
        let cover_path = format!("file://{}", path);

        cover_path
    }

    fn update_discord_song(&self, force_position_ms: Option<i64>) {
        let song_data = match &self.current_song {
            Some(song) => (song.clone(), self.duration),
            None => return,
        };

        let pos_ms = force_position_ms
            .unwrap_or_else(|| i64::try_from(self.player.get_pos().as_millis()).unwrap_or(0));
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
        let prev_pos = self.seek_offset + self.player.get_pos();
        let prev_song = self.current_song.clone();
        // Prepared streams belong to nothing device specific, but their decoders are cheap to reopen.
        self.prepared = None;

        let new_sink = Self::create_sink()?;
        let new_player = Player::connect_new(new_sink.mixer());

        self.sink = new_sink;
        self.player = new_player;

        if let Some(song) = prev_song.as_ref().filter(|s| is_stream_song(s)).cloned() {
            // A stream is reopened from the network (from the start).
            self.play(song);
        } else if let Some(song) = prev_song {
            if let Ok(mut source) = Self::decode_source(&song.path) {
                if prev_pos.as_millis() > 0 {
                    let _ = source.try_seek(prev_pos);

                    self.seek_offset = prev_pos;
                } else {
                    self.seek_offset = Duration::ZERO;
                }

                self.player.append(source);
                self.set_volume(prev_volume);

                if prev_is_paused {
                    self.player.pause();
                } else {
                    self.player.play();
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

    // ----- streaming -----

    /// Plays a stream song: right away when it was prepared, otherwise it is opened in the background
    /// (`stream_loading` is set meanwhile) and starts when ready.
    fn play_stream_song(&mut self, song: Song) {
        let url = song.source_url.clone().unwrap_or_default();
        if let Some((source, duration)) = self.take_prepared(&url) {
            self.stream_loading = None;
            return self.start_source(song, source, duration);
        }
        self.stream_generation += 1;
        let generation = self.stream_generation;
        self.player.clear();
        self.seek_offset = Duration::ZERO;
        self.duration = Some(Duration::from_millis(song.duration_ms));
        self.stream_loading = Some(url.clone());
        self.current_song = Some(song.clone());
        self.update_current_metadata();
        self.emit_state();

        let handle = self.handle.clone();
        let platform = song.source;
        tauri::async_runtime::spawn(async move {
            let result = crate::preview::commands::open_stream(&handle, platform, &url).await;
            let state = handle.state::<Mutex<AudioPlayer>>();
            let mut player = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            player.on_stream_opened(generation, &url, result);
        });
    }

    fn on_stream_opened(
        &mut self,
        generation: u64,
        url: &str,
        result: Result<(BoxedSource, Option<Duration>), String>,
    ) {
        if generation != self.stream_generation || self.stream_loading.as_deref() != Some(url) {
            return; // a newer song was started meanwhile
        }
        self.stream_loading = None;
        let Some(song) = self.current_song.clone() else {
            return;
        };
        match result {
            Ok((source, duration)) => self.start_source(song, source, duration),
            Err(message) => {
                eprintln!("[stream] could not open {url}: {message}");
                let _ = self.handle.emit(
                    "stream-error",
                    serde_json::json!({ "url": url, "message": message }),
                );
                self.stop();
            }
        }
    }

    /// Starts an opened source as the current song.
    fn start_source(&mut self, song: Song, source: BoxedSource, duration: Option<Duration>) {
        self.duration = source
            .total_duration()
            .or(duration)
            .or_else(|| Some(Duration::from_millis(song.duration_ms)));
        self.seek_offset = Duration::ZERO;
        self.player.clear();
        self.player.append(source);
        self.player.play();
        self.current_song = Some(song);
        self.adopt_stream_duration();

        self.update_current_metadata();
        let _ = self.media_controls.play();
        self.emit_state();
        self.update_discord_song(Some(0));
        self.prepare_next();
    }

    /// Search results do not always know a track's length (Bandcamp never does), so a stream song may start
    /// with `duration_ms == 0`. Once the decoder or the platform told us, copy it into the current song and its
    /// queue entries so the player and the queue show it.
    fn adopt_stream_duration(&mut self) {
        let Some(duration) = self.duration else {
            return;
        };
        let ms = duration.as_millis() as u64;
        let Some(song) = self.current_song.as_mut() else {
            return;
        };
        if ms == 0 || song.duration_ms != 0 || !is_stream_song(song) {
            return;
        }
        song.duration_ms = ms;
        let url = song.source_url.clone();
        for entry in self.queue.iter_mut() {
            if entry.duration_ms == 0 && is_stream_song(entry) && entry.source_url == url {
                entry.duration_ms = ms;
            }
        }
    }

    /// The prepared source of `url`, if that is the one waiting.
    fn take_prepared(&mut self, url: &str) -> Option<(BoxedSource, Option<Duration>)> {
        match self.prepared.take() {
            Some((prepared_url, source, duration)) if prepared_url == url => Some((source, duration)),
            other => {
                self.prepared = other;
                None
            }
        }
    }

    /// Opens the next song of the queue in the background when it is a stream, so the transition has no
    /// network delay. Only one song is kept prepared.
    fn prepare_next(&mut self) {
        let Some(next) = self.get_next_cursor().and_then(|c| self.song_at(c)) else {
            return;
        };
        if !is_stream_song(&next) {
            return;
        }
        let url = next.source_url.clone().unwrap_or_default();
        let already = self.prepared.as_ref().is_some_and(|(u, _, _)| *u == url)
            || self.preparing.as_deref() == Some(url.as_str());
        if already {
            return;
        }
        self.prepared = None;
        self.preparing = Some(url.clone());
        let handle = self.handle.clone();
        let platform = next.source;
        tauri::async_runtime::spawn(async move {
            let result = crate::preview::commands::open_stream(&handle, platform, &url).await;
            let state = handle.state::<Mutex<AudioPlayer>>();
            let mut player = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if player.preparing.as_deref() != Some(url.as_str()) {
                return;
            }
            player.preparing = None;
            match result {
                Ok((source, duration)) => player.prepared = Some((url, source, duration)),
                Err(e) => eprintln!("[stream] could not prepare {url}: {e}"),
            }
        });
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
                                let current_pos =
                                    i64::try_from(player.player.get_pos().as_millis()).unwrap_or(0);
                                player.update_discord_song(Some(current_pos));
                            }
                        }
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_song_edit, is_stream_song, source_index_at, AudioPlayer};
    use crate::types::{Platform, QualityTier, Song};
    use rodio::Source;
    use std::time::Duration;

    fn make_song(id: Option<i64>, title: &str) -> Song {
        Song {
            id,
            path: "/music/track.opus".to_string(),
            title: title.to_string(),
            artists: vec!["Artist A".to_string()],
            album_artist: "Album Artist".to_string(),
            album: "Album".to_string(),
            duration_ms: 180_000,
            track_number: Some(1),
            genre: None,
            release_year: None,
            cover_url: Some("/covers/a.jpg".to_string()),
            external_cover_url: None,
            lyrics: Some("original lyrics".to_string()),
            lyrics_source: None,
            source: Platform::Local,
            source_url: None,
            source_item_id: None,
            canonical_track_slug: "artist-a-title".to_string(),
            canonical_album_slug: "artist-a-album".to_string(),
            quality_tier: QualityTier::Standard,
        }
    }

    #[test]
    fn apply_song_edit_same_id_copies_editable_fields_and_keeps_playback_fields() {
        let mut target = make_song(Some(7), "Old Title");
        let mut updated = make_song(Some(7), "New Title");
        updated.artists = vec!["Artist B".to_string(), "Artist C".to_string()];
        updated.album = "New Album".to_string();
        updated.album_artist = "New Album Artist".to_string();
        updated.track_number = Some(5);
        updated.canonical_track_slug = "artist-b-new-title".to_string();
        updated.canonical_album_slug = "artist-b-new-album".to_string();
        updated.path = "/music/other.mp3".to_string();
        updated.duration_ms = 1;
        updated.cover_url = Some("/covers/b.jpg".to_string());
        updated.lyrics = Some("new lyrics".to_string());

        let applied = apply_song_edit(&mut target, &updated);

        assert!(applied);
        assert_eq!(target.title, "New Title");
        assert_eq!(target.artists, vec!["Artist B", "Artist C"]);
        assert_eq!(target.album, "New Album");
        assert_eq!(target.album_artist, "New Album Artist");
        assert_eq!(target.track_number, Some(5));
        assert_eq!(target.canonical_track_slug, "artist-b-new-title");
        assert_eq!(target.canonical_album_slug, "artist-b-new-album");

        assert_eq!(target.path, "/music/track.opus");
        assert_eq!(target.duration_ms, 180_000);
        assert_eq!(target.cover_url.as_deref(), Some("/covers/a.jpg"));
        assert_eq!(target.lyrics.as_deref(), Some("original lyrics"));
        assert_eq!(target.id, Some(7));
    }

    #[test]
    fn apply_song_edit_different_id_returns_false_and_leaves_target_unchanged() {
        let mut target = make_song(Some(7), "Old Title");
        let before = target.clone();
        let updated = make_song(Some(8), "New Title");

        let applied = apply_song_edit(&mut target, &updated);

        assert!(!applied);
        assert_eq!(target.id, before.id);
        assert_eq!(target.title, before.title);
        assert_eq!(target.artists, before.artists);
        assert_eq!(target.album, before.album);
        assert_eq!(target.album_artist, before.album_artist);
        assert_eq!(target.track_number, before.track_number);
        assert_eq!(target.canonical_track_slug, before.canonical_track_slug);
        assert_eq!(target.canonical_album_slug, before.canonical_album_slug);
        assert_eq!(target.path, before.path);
        assert_eq!(target.duration_ms, before.duration_ms);
        assert_eq!(target.cover_url, before.cover_url);
        assert_eq!(target.lyrics, before.lyrics);
    }

    #[test]
    fn apply_song_edit_target_without_id_returns_false_and_leaves_target_unchanged() {
        let mut target = make_song(None, "Old Title");
        let before = target.clone();
        let updated = make_song(Some(7), "New Title");

        let applied = apply_song_edit(&mut target, &updated);

        assert!(!applied);
        assert_eq!(target.id, None);
        assert_eq!(target.title, before.title);
        assert_eq!(target.artists, before.artists);
        assert_eq!(target.album, before.album);
        assert_eq!(target.album_artist, before.album_artist);
        assert_eq!(target.track_number, before.track_number);
        assert_eq!(target.canonical_track_slug, before.canonical_track_slug);
        assert_eq!(target.canonical_album_slug, before.canonical_album_slug);
    }

    /// Builds a minimal valid 16-bit mono PCM WAV containing `seconds` of silence.
    fn silent_wav_bytes(sample_rate: u32, seconds: u32) -> Vec<u8> {
        let channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let block_align = channels * bits_per_sample / 8;
        let byte_rate = sample_rate * block_align as u32;
        let data_len = byte_rate * seconds;

        let mut buf = Vec::with_capacity(44 + data_len as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_len).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.resize(44 + data_len as usize, 0);
        buf
    }

    #[test]
    fn decode_from_memory_wav_decodes_and_seeks() {
        let bytes = silent_wav_bytes(8000, 2);

        let mut source = AudioPlayer::decode_source_from_memory("x.wav", bytes)
            .expect("valid in-memory WAV should decode");

        source
            .try_seek(Duration::from_millis(500))
            .expect("seeking a seekable in-memory WAV should succeed");

        let produced = source.take(100).count();
        assert!(produced >= 1, "expected at least one sample after seeking, got {produced}");
    }

    #[test]
    fn decode_from_memory_rejects_garbage() {
        let result = AudioPlayer::decode_source_from_memory("x.wav", b"not audio".to_vec());

        assert!(result.is_err());
    }

    #[test]
    fn stream_song_has_no_path_but_a_source_url() {
        let mut song = make_song(None, "s");
        song.path = String::new();
        song.source_url = Some("https://soundcloud.com/a/b".into());
        assert!(is_stream_song(&song));
        song.source_url = Some(String::new());
        assert!(!is_stream_song(&song), "empty url is not a stream");
        song.source_url = None;
        assert!(!is_stream_song(&song));
        let mut file = make_song(Some(1), "f");
        file.path = "C:/music/a.mp3".into();
        file.source_url = Some("https://youtube.com/watch?v=x".into());
        assert!(!is_stream_song(&file), "downloaded songs keep their source url but play from the file");
    }

    #[test]
    fn source_index_at_none_play_order_returns_none() {
        assert_eq!(source_index_at(None, Some(0)), None);
        assert_eq!(source_index_at(None, None), None);
    }

    #[test]
    fn source_index_at_none_cursor_returns_none() {
        let order = [2usize, 0, 1];
        assert_eq!(source_index_at(Some(&order), None), None);
    }

    #[test]
    fn source_index_at_returns_order_entry_for_cursor() {
        let order = [2usize, 0, 1];
        assert_eq!(source_index_at(Some(&order), Some(0)), Some(2));
        assert_eq!(source_index_at(Some(&order), Some(1)), Some(0));
        assert_eq!(source_index_at(Some(&order), Some(2)), Some(1));
    }

    #[test]
    fn source_index_at_cursor_out_of_range_returns_none() {
        let order = [2usize, 0, 1];
        assert_eq!(source_index_at(Some(&order), Some(3)), None);
        assert_eq!(source_index_at(Some(&order), Some(usize::MAX)), None);
    }

    #[test]
    fn source_index_at_empty_order_returns_none() {
        let order: [usize; 0] = [];
        assert_eq!(source_index_at(Some(&order), Some(0)), None);
        assert_eq!(source_index_at(Some(&order), None), None);
    }

    #[test]
    fn source_index_at_identity_order_returns_cursor() {
        let order: Vec<usize> = (0..5).collect();
        for cursor in 0..5 {
            assert_eq!(source_index_at(Some(&order), Some(cursor)), Some(cursor));
        }
    }
}
