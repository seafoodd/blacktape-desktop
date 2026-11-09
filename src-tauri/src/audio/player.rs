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

fn source_key(song: &Song) -> Option<String> {
    song.source_url.as_deref().and_then(crate::library_index::normalize::normalize_url_key)
}

/// The downloaded file (id and path set) of the same track as the stream song `song`, matched by normalized
/// source url.
pub fn file_for_stream<'a>(song: &Song, files: &'a [Song]) -> Option<&'a Song> {
    if !is_stream_song(song) {
        return None;
    }
    let url = source_key(song)?;
    files
        .iter()
        .find(|f| f.id.is_some() && !f.path.is_empty() && source_key(f).as_deref() == Some(url.as_str()))
}

/// A seek fraction from the UI, forced into 0..=1 (`Duration::mul_f32` panics on NaN or a negative factor).
pub fn clamp_fraction(fraction: f32) -> f32 {
    if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) }
}

/// How far from the target a position may be after a seek and still count as having landed there.
const SEEK_TOLERANCE: Duration = Duration::from_millis(1500);

/// Ok when the playing position `landed` is within `SEEK_TOLERANCE` of `target`; else why not.
pub fn check_seek_landed(target: Duration, landed: Duration) -> Result<(), String> {
    let off = if landed > target { landed - target } else { target - landed };
    if off <= SEEK_TOLERANCE {
        Ok(())
    } else {
        Err(format!("the position is {landed:?} instead of {target:?}"))
    }
}

/// True when a downloaded file (id and path set) has the same normalized source url as `url`.
fn file_for_stream_url(url: &str, files: &[Song]) -> bool {
    let Some(key) = crate::library_index::normalize::normalize_url_key(url) else {
        return false;
    };
    files.iter().any(|f| f.id.is_some() && !f.path.is_empty() && source_key(f).as_deref() == Some(key.as_str()))
}

/// The stream version of a library song whose file is gone, when it has a source page to stream from.
pub fn stream_fallback(song: &Song, file_exists: bool) -> Option<Song> {
    if is_stream_song(song) || file_exists || song.source_url.as_deref().map_or(true, str::is_empty) {
        return None;
    }
    let mut stream = song.clone();
    stream.id = None;
    stream.path = String::new();
    Some(stream)
}

/// Turns queued library songs that were deleted into stream songs of the same track, so the queue keeps its
/// order and the track plays from its page. Positions in `keep` (the current song) and songs without a source url
/// are left alone. Returns how many were replaced.
pub fn streamify_deleted(queue: &mut [Song], keep: &[usize], deleted: &[Song]) -> usize {
    let mut replaced = 0;
    for (i, queued) in queue.iter_mut().enumerate() {
        if keep.contains(&i) || is_stream_song(queued) {
            continue;
        }
        let Some(id) = queued.id else { continue };
        let Some(gone) = deleted.iter().find(|d| d.id == Some(id)) else { continue };
        if gone.source_url.as_deref().map_or(true, str::is_empty) {
            continue;
        }
        let mut stream = gone.clone();
        stream.id = None;
        stream.path = String::new();
        *queued = stream;
        replaced += 1;
    }
    replaced
}

/// Replaces queued stream songs by the downloaded files of the same track (matched by normalized source url).
/// Queue positions listed in `keep` (a stream that is opening or prepared) are left alone. Returns how many were
/// swapped.
pub fn swap_streams_for_files(queue: &mut [Song], keep: &[usize], files: &[Song]) -> usize {
    let mut swapped = 0;
    for (i, queued) in queue.iter_mut().enumerate() {
        if keep.contains(&i) {
            continue;
        }
        if let Some(file) = file_for_stream(queued, files) {
            *queued = file.clone();
            swapped += 1;
        }
    }
    swapped
}

/// Appends queue index `new_idx` at the end of the play order. An empty player starts at cursor 0.
fn order_push(order: &mut Option<Vec<usize>>, cursor: &mut Option<usize>, new_idx: usize) {
    if let Some(order) = order {
        order.push(new_idx);
    } else {
        *order = Some(vec![new_idx]);
        *cursor = Some(0);
    }
}

/// Inserts queue index `new_idx` right after the cursor ("play next").
fn order_insert_next(order: &mut Option<Vec<usize>>, cursor: &mut Option<usize>, new_idx: usize) {
    match (order.as_mut(), *cursor) {
        (Some(order), Some(cursor_pos)) => {
            let insert_pos = (cursor_pos + 1).min(order.len());
            order.insert(insert_pos, new_idx);
        }
        (Some(order), None) => {
            order.push(new_idx);
            *cursor = Some(0);
        }
        (None, _) => {
            *order = Some(vec![new_idx]);
            *cursor = Some(0);
        }
    }
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
    /// Id of the current song when it was swapped for its downloaded file while its stream keeps playing: the UI
    /// sees a local song, the audio is still the stream.
    stream_backing_id: Option<i64>,
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
            stream_backing_id: None,
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
                            && player.current_is_stream_audio();
                        if stream_ran_out {
                            // A stream ended before its announced duration (or nothing was buffered).
                            if let Some(next_cursor) = player.get_next_cursor() {
                                if let Some(next_song) = player.song_at(next_cursor) {
                                    pending_transition = Some((next_cursor, next_song, false));
                                }
                            }
                            sleep_time = Duration::ZERO;
                            advanced = true;
                        } else if let Some(duration) = player.duration.filter(|d| !d.is_zero()) {
                            // (an unknown duration is not "already over": a stream of unknown length ends by running dry)
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
                                .or(Some(Duration::from_millis(next_song.duration_ms)))
                                .filter(|d| !d.is_zero());
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
        self.stream_backing_id = None;
        if is_stream_song(&song) {
            return self.play_stream_song(song);
        }
        // the file is gone (deleted): play the same track from its page and fix the queue entry
        if let Some(stream) = stream_fallback(&song, Path::new(&song.path).exists()) {
            if let Some(slot) = self.play_order.as_ref().zip(self.cursor).and_then(|(o, c)| o.get(c).copied()) {
                if let Some(entry) = self.queue.get_mut(slot) {
                    if entry.id == song.id {
                        *entry = stream.clone();
                    }
                }
            }
            return self.play_stream_song(stream);
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
        self.add_songs_to_queue(vec![song], false);
    }

    pub fn add_next_to_queue(&mut self, song: Song) {
        self.add_songs_to_queue(vec![song], true);
    }

    /// Adds `songs` without replacing the queue: right after the current song (keeping the given order)
    /// when `play_next`, at the end otherwise. One state emit and one `prepare_next` for the whole batch.
    pub fn add_songs_to_queue(&mut self, songs: Vec<Song>, play_next: bool) {
        if songs.is_empty() {
            return;
        }
        if play_next {
            // Every insert lands at cursor + 1, so going backwards leaves the songs in the given order.
            for song in songs.into_iter().rev() {
                self.queue.push(song);
                let new_idx = self.queue.len() - 1;
                order_insert_next(&mut self.play_order, &mut self.cursor, new_idx);
            }
        } else {
            for song in songs {
                self.queue.push(song);
                let new_idx = self.queue.len() - 1;
                order_push(&mut self.play_order, &mut self.cursor, new_idx);
            }
        }
        // The song after the current one may have changed (or just appeared): a stream that was prepared for
        // the old next song is stale, so re-evaluate. No-op when the next song is a file or already prepared.
        if self.current_song.is_some() {
            self.prepare_next();
        }
        self.emit_state();
    }

    /// Number of songs in the queue.
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Whether shuffle mode is on.
    pub fn is_shuffle(&self) -> bool {
        self.shuffle_mode
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
        self.stream_missing_file_at(next_cursor);

        if let Some(next_song) = self.get_song_at_cursor() {
            if is_stream_song(&next_song) {
                self.play(next_song);
                return true;
            }
            // a stream that was still opening is abandoned (its late result is ignored)
            self.stream_loading = None;
            self.stream_backing_id = None;
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
        self.stream_missing_file_at(next_cursor);
        let next_song = self.song_at(next_cursor)?;
        let appended = self.buffer_next_silent(&next_song);
        Some((next_song, appended))
    }

    /// If the library song at play-order position `cursor` has no file any more (deleted), its queue entry becomes
    /// a stream of the same track (when it has a source page).
    fn stream_missing_file_at(&mut self, cursor: usize) {
        let Some(slot) = self.play_order.as_ref().and_then(|o| o.get(cursor).copied()) else {
            return;
        };
        let Some(song) = self.queue.get(slot) else {
            return;
        };
        if let Some(stream) = stream_fallback(song, Path::new(&song.path).exists()) {
            self.queue[slot] = stream;
        }
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

    /// Seeks the current song to `fraction` (0..=1) of its length. The result says whether the position really
    /// moved there: rodio reports `Ok` also when nothing was playing (the request is only stored), so the position
    /// is read back and compared with the target. Every step is logged with the `[seek]` prefix.
    pub fn seek(&mut self, fraction: f32) -> Result<(), String> {
        let started = Instant::now();
        let fraction = clamp_fraction(fraction);
        let kind = self.current_source_kind();
        let before = self.seek_offset + self.player.get_pos();
        println!(
            "[seek] request fraction={fraction:.4} song={:?} kind={kind} paused={} queued_sources={} pos_before={before:?} stream_loading={:?}",
            self.current_song.as_ref().map(|s| s.title.as_str()),
            self.player.is_paused(),
            self.player.len(),
            self.stream_loading,
        );

        let duration = self.duration.filter(|d| !d.is_zero()).or_else(|| {
            self.current_song
                .as_ref()
                .map(|s| Duration::from_millis(s.duration_ms))
                .filter(|d| !d.is_zero())
        });
        let Some(duration) = duration else {
            let msg = "seek ignored: the song's length is unknown".to_string();
            eprintln!("[seek] FAILED: {msg}");
            return Err(msg);
        };
        self.duration = Some(duration);
        let target = duration.mul_f32(fraction);
        let remaining = duration.saturating_sub(target);

        if self.player.len() < 1 && self.stream_loading.is_some() {
            let msg = "seek ignored: the stream is still loading".to_string();
            eprintln!("[seek] FAILED: {msg}");
            return Err(msg);
        }
        if self.player.len() < 1 {
            println!("[seek] nothing is queued in the player: reloading the current song");
            if let Some(song) = self.current_song.clone() {
                self.play(song);
                return Ok(());
            }
            let msg = "seek ignored: nothing is playing".to_string();
            eprintln!("[seek] FAILED: {msg}");
            return Err(msg);
        }

        if remaining < Duration::from_secs(1) {
            println!("[seek] target {target:?} is within 1 s of the end ({duration:?}): skipping to the next song");
            let Some(next_cursor) = self.get_next_cursor() else {
                self.next();
                return Ok(());
            };

            self.player.skip_one();
            self.advance_to_next_in_queue(next_cursor);
            self.emit_state();
            self.update_current_metadata();
            return Ok(());
        }

        let seek_started = Instant::now();
        let result = self.player.try_seek(target);
        let seek_took = seek_started.elapsed();
        if let Err(e) = result {
            let msg = format!("the source cannot seek to {target:?} ({kind}): {e:?}");
            eprintln!("[seek] FAILED: {msg} (try_seek took {seek_took:?})");
            return Err(msg);
        }

        // rodio's tracker is absolute after a seek
        let landed = self.player.get_pos();
        if let Err(msg) = check_seek_landed(target, landed) {
            eprintln!(
                "[seek] FAILED: try_seek returned Ok but {msg} ({kind}, queued_sources={}, try_seek took {seek_took:?})",
                self.player.len()
            );
            return Err(msg);
        }
        self.seek_offset = Duration::ZERO;
        println!(
            "[seek] ok: target={target:?} landed={landed:?} duration={duration:?} try_seek={seek_took:?} total={:?}",
            started.elapsed()
        );

        let target_ms = i64::try_from(target.as_millis()).unwrap_or(0);
        self.update_discord_song(Some(target_ms));
        self.update_current_metadata();
        self.emit_state();
        Ok(())
    }

    /// What the audio of the current song comes from, for the logs.
    fn current_source_kind(&self) -> &'static str {
        match self.current_song.as_ref() {
            None => "none",
            Some(s) if self.current_is_stream_audio() => {
                if is_stream_song(s) { "stream" } else { "stream(shown as file)" }
            }
            Some(s) if s.path.to_ascii_lowercase().ends_with(".opus") => "opus-file",
            Some(_) => "file",
        }
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

    /// Library songs were deleted: the ones in the queue (except the current one) become streams of the same
    /// track. Returns how many were replaced.
    pub fn streamify_deleted(&mut self, deleted: &[Song]) -> usize {
        let current = self
            .play_order
            .as_ref()
            .zip(self.cursor)
            .and_then(|(order, c)| order.get(c).copied());
        let keep: Vec<usize> = current.into_iter().collect();
        let replaced = streamify_deleted(&mut self.queue, &keep, deleted);
        if replaced > 0 {
            self.prepare_next();
            self.emit_state();
        }
        replaced
    }

    /// True when the audio of the current song is a stream: a stream song, or a song swapped for its file while
    /// the stream keeps playing.
    fn current_is_stream_audio(&self) -> bool {
        self.current_song
            .as_ref()
            .is_some_and(|s| is_stream_song(s) || (s.id.is_some() && s.id == self.stream_backing_id))
    }

    /// Swaps stream songs for their downloaded files: the queued ones, and the current one in the UI only (the
    /// stream keeps playing underneath, see `stream_backing_id`; it is not reopened from the file). A stream that is
    /// opening or prepared as the next song stays as it is. Returns how many songs were swapped.
    pub fn swap_streams_for_downloads(&mut self, files: &[Song]) -> usize {
        // only the stream that is opening right now stays; a prepared / preparing next song is swapped too
        let busy: Vec<String> = self.stream_loading.clone().into_iter().collect();
        let keep: Vec<usize> = self
            .queue
            .iter()
            .enumerate()
            .filter(|(_, queued)| queued.source_url.as_ref().is_some_and(|u| busy.contains(u)))
            .map(|(i, _)| i)
            .collect();
        let mut swapped = swap_streams_for_files(&mut self.queue, &keep, files);
        if swapped > 0 {
            // a stream prepared for a song that is a file now is no longer the next one
            let gone = |url: &str| file_for_stream_url(url, files);
            if self.prepared.as_ref().is_some_and(|(u, _, _)| gone(u)) {
                self.prepared = None;
            }
            if self.preparing.as_deref().is_some_and(gone) {
                self.preparing = None;
            }
        }

        if self.stream_loading.is_none() {
            let file = self
                .current_song
                .as_ref()
                .and_then(|current| file_for_stream(current, files))
                .cloned();
            if let Some(file) = file {
                self.stream_backing_id = file.id;
                self.current_song = Some(file);
                self.update_current_metadata();
                swapped += 1;
            }
        }
        if swapped > 0 {
            self.emit_state();
        }
        swapped
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
    use super::{
        apply_song_edit, check_seek_landed, clamp_fraction, file_for_stream, is_stream_song, order_insert_next,
        order_push, source_index_at, stream_fallback, streamify_deleted, swap_streams_for_files, AudioPlayer,
    };
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

    const STREAM_URL: &str = "https://soundcloud.com/artist/track";

    fn stream_song(url: &str) -> Song {
        let mut song = make_song(None, "stream");
        song.path = String::new();
        song.source_url = Some(url.to_string());
        song
    }

    fn library_file(id: i64, path: &str, url: Option<&str>) -> Song {
        let mut song = make_song(Some(id), "file");
        song.path = path.to_string();
        song.source_url = url.map(str::to_string);
        song
    }

    #[test]
    fn swap_streams_for_files_replaces_stream_with_file_of_same_source_url() {
        let mut queue = vec![stream_song(STREAM_URL)];
        let files = vec![library_file(5, "C:/music/track.mp3", Some(STREAM_URL))];

        let swapped = swap_streams_for_files(&mut queue, &[], &files);

        assert_eq!(swapped, 1);
        assert_eq!(queue[0].path, "C:/music/track.mp3");
        assert_eq!(queue[0].id, Some(5));
        assert!(!is_stream_song(&queue[0]));
    }

    #[test]
    fn swap_streams_for_files_matches_urls_that_differ_only_by_host_prefix_scheme_or_query() {
        let file = library_file(1, "C:/music/a.opus", Some("https://www.youtube.com/watch?v=abc"));

        let mut queue = vec![stream_song("https://music.youtube.com/watch?v=abc&list=x")];
        assert_eq!(swap_streams_for_files(&mut queue, &[], std::slice::from_ref(&file)), 1);
        assert_eq!(queue[0].path, "C:/music/a.opus");

        let mut queue = vec![stream_song("http://youtube.com/watch?v=abc")];
        assert_eq!(swap_streams_for_files(&mut queue, &[], std::slice::from_ref(&file)), 1);
        assert_eq!(queue[0].path, "C:/music/a.opus");
    }

    #[test]
    fn swap_streams_for_files_does_not_match_a_different_track() {
        let mut queue = vec![stream_song("https://www.youtube.com/watch?v=other")];
        let files = vec![library_file(1, "C:/music/a.opus", Some("https://www.youtube.com/watch?v=abc"))];

        assert_eq!(swap_streams_for_files(&mut queue, &[], &files), 0);
        assert!(is_stream_song(&queue[0]));
    }

    #[test]
    fn swap_streams_for_files_returns_the_number_of_swapped_streams() {
        let one = "https://soundcloud.com/artist/one";
        let two = "https://soundcloud.com/artist/two";
        let three = "https://soundcloud.com/artist/three";
        let mut queue = vec![stream_song(one), stream_song(two), stream_song(three)];
        let files = vec![
            library_file(1, "C:/music/one.mp3", Some(one)),
            library_file(2, "C:/music/three.mp3", Some(three)),
        ];

        assert_eq!(swap_streams_for_files(&mut queue, &[], &files), 2);
        assert_eq!(queue[0].path, "C:/music/one.mp3");
        assert!(is_stream_song(&queue[1]));
        assert_eq!(queue[2].path, "C:/music/three.mp3");
    }

    #[test]
    fn swap_streams_for_files_leaves_kept_positions_alone() {
        let mut queue = vec![stream_song(STREAM_URL), stream_song(STREAM_URL)];
        let files = vec![library_file(5, "C:/music/track.mp3", Some(STREAM_URL))];

        assert_eq!(swap_streams_for_files(&mut queue, &[0], &files), 1);
        assert!(is_stream_song(&queue[0]), "kept position must stay a stream");
        assert_eq!(queue[1].path, "C:/music/track.mp3");
    }

    #[test]
    fn swap_streams_for_files_never_touches_local_songs() {
        let mut local = make_song(Some(9), "local");
        local.source_url = Some(STREAM_URL.to_string());
        let mut queue = vec![local];
        let files = vec![library_file(5, "C:/music/track.mp3", Some(STREAM_URL))];

        assert_eq!(swap_streams_for_files(&mut queue, &[], &files), 0);
        assert_eq!(queue[0].path, "/music/track.opus");
        assert_eq!(queue[0].id, Some(9));
    }

    #[test]
    fn swap_streams_for_files_ignores_files_without_id_or_path() {
        let mut queue = vec![stream_song(STREAM_URL)];
        let mut without_id = library_file(1, "C:/music/track.mp3", Some(STREAM_URL));
        without_id.id = None;
        let without_path = library_file(2, "", Some(STREAM_URL));

        assert_eq!(swap_streams_for_files(&mut queue, &[], &[without_id, without_path]), 0);
        assert!(is_stream_song(&queue[0]));
    }

    #[test]
    fn swap_streams_for_files_without_a_matching_file_leaves_queue_unchanged() {
        let mut queue = vec![stream_song(STREAM_URL)];
        let files = vec![library_file(1, "C:/music/other.mp3", Some("https://soundcloud.com/artist/other"))];

        assert_eq!(swap_streams_for_files(&mut queue, &[], &files), 0);
        assert!(is_stream_song(&queue[0]));
        assert_eq!(queue[0].source_url.as_deref(), Some(STREAM_URL));
    }

    fn library_song_with_url(id: i64, title: &str, url: Option<&str>) -> Song {
        let mut song = make_song(Some(id), title);
        song.path = format!("C:/music/{id}.mp3");
        song.source_url = url.map(str::to_string);
        song
    }

    #[test]
    fn streamify_deleted_turns_a_deleted_library_song_into_a_stream_of_the_same_track() {
        let mut queue = vec![library_song_with_url(5, "Gone", Some(STREAM_URL))];
        let deleted = vec![library_song_with_url(5, "Gone", Some(STREAM_URL))];

        let replaced = streamify_deleted(&mut queue, &[], &deleted);

        assert_eq!(replaced, 1);
        assert_eq!(queue[0].id, None);
        assert!(queue[0].path.is_empty());
        assert_eq!(queue[0].title, "Gone");
        assert_eq!(queue[0].source_url.as_deref(), Some(STREAM_URL));
        assert!(is_stream_song(&queue[0]));
    }

    #[test]
    fn streamify_deleted_leaves_kept_positions_alone() {
        let mut queue = vec![library_song_with_url(5, "Playing", Some(STREAM_URL)), library_song_with_url(5, "Next", Some(STREAM_URL))];
        let deleted = vec![library_song_with_url(5, "Playing", Some(STREAM_URL))];

        let replaced = streamify_deleted(&mut queue, &[0], &deleted);

        assert_eq!(replaced, 1);
        assert_eq!(queue[0].id, Some(5), "kept position must stay a library song");
        assert_eq!(queue[0].path, "C:/music/5.mp3");
        assert!(is_stream_song(&queue[1]));
    }

    #[test]
    fn streamify_deleted_keeps_deleted_songs_without_a_source_url() {
        let mut queue = vec![library_song_with_url(5, "No url", None), library_song_with_url(6, "Empty url", Some(""))];
        let deleted = vec![library_song_with_url(5, "No url", None), library_song_with_url(6, "Empty url", Some(""))];

        let replaced = streamify_deleted(&mut queue, &[], &deleted);

        assert_eq!(replaced, 0);
        assert_eq!(queue[0].id, Some(5));
        assert_eq!(queue[0].path, "C:/music/5.mp3");
        assert_eq!(queue[1].id, Some(6));
        assert_eq!(queue[1].path, "C:/music/6.mp3");
    }

    #[test]
    fn streamify_deleted_leaves_queued_songs_that_were_not_deleted_untouched() {
        let mut queue = vec![library_song_with_url(3, "Kept", Some(STREAM_URL))];
        let deleted = vec![library_song_with_url(4, "Other", Some(STREAM_URL))];

        let replaced = streamify_deleted(&mut queue, &[], &deleted);

        assert_eq!(replaced, 0);
        assert_eq!(queue[0].id, Some(3));
        assert_eq!(queue[0].path, "C:/music/3.mp3");
        assert!(!is_stream_song(&queue[0]));
    }

    #[test]
    fn streamify_deleted_leaves_existing_stream_songs_untouched() {
        let mut queue = vec![stream_song(STREAM_URL)];
        let deleted = vec![library_song_with_url(5, "Gone", Some(STREAM_URL))];

        let replaced = streamify_deleted(&mut queue, &[], &deleted);

        assert_eq!(replaced, 0);
        assert!(is_stream_song(&queue[0]));
        assert_eq!(queue[0].id, None);
        assert_eq!(queue[0].title, "stream");
        assert_eq!(queue[0].source_url.as_deref(), Some(STREAM_URL));
    }

    #[test]
    fn streamify_deleted_returns_the_count_and_preserves_queue_order() {
        let one = "https://soundcloud.com/artist/one";
        let two = "https://soundcloud.com/artist/two";
        let mut queue = vec![
            library_song_with_url(1, "First", Some(one)),
            library_song_with_url(2, "Second", Some(two)),
            library_song_with_url(3, "Third", Some(one)),
            stream_song(STREAM_URL),
        ];
        let deleted = vec![library_song_with_url(1, "First", Some(one)), library_song_with_url(3, "Third", Some(one))];

        let replaced = streamify_deleted(&mut queue, &[], &deleted);

        assert_eq!(replaced, 2);
        assert_eq!(queue.len(), 4);
        assert_eq!(queue[0].title, "First");
        assert!(is_stream_song(&queue[0]));
        assert_eq!(queue[1].id, Some(2));
        assert_eq!(queue[1].title, "Second");
        assert_eq!(queue[2].title, "Third");
        assert!(is_stream_song(&queue[2]));
        assert_eq!(queue[3].source_url.as_deref(), Some(STREAM_URL));
    }

    #[test]
    fn stream_fallback_turns_a_missing_library_song_into_a_stream_of_the_same_track() {
        let song = library_song_with_url(5, "Gone", Some(STREAM_URL));

        let stream = stream_fallback(&song, false).expect("missing file with a source page should stream");

        assert_eq!(stream.id, None);
        assert!(stream.path.is_empty());
        assert_eq!(stream.title, "Gone");
        assert_eq!(stream.source_url.as_deref(), Some(STREAM_URL));
        assert!(is_stream_song(&stream));
    }

    #[test]
    fn stream_fallback_keeps_a_library_song_whose_file_exists() {
        let song = library_song_with_url(5, "Here", Some(STREAM_URL));

        assert!(stream_fallback(&song, true).is_none());
    }

    #[test]
    fn stream_fallback_needs_a_source_url() {
        let no_url = library_song_with_url(5, "No url", None);
        let empty_url = library_song_with_url(6, "Empty url", Some(""));

        assert!(stream_fallback(&no_url, false).is_none());
        assert!(stream_fallback(&empty_url, false).is_none());
    }

    #[test]
    fn stream_fallback_leaves_existing_stream_songs_alone() {
        let stream = stream_song(STREAM_URL);

        assert!(stream_fallback(&stream, false).is_none());
    }

    #[test]
    fn file_for_stream_returns_the_file_with_the_same_normalized_url() {
        let file = library_file(1, "C:/music/a.opus", Some("https://www.youtube.com/watch?v=abc"));
        let stream = stream_song("https://music.youtube.com/watch?v=abc&list=x");
        let files = vec![file];

        let found = file_for_stream(&stream, &files).expect("same track should match");

        assert_eq!(found.id, Some(1));
        assert_eq!(found.path, "C:/music/a.opus");
    }

    #[test]
    fn file_for_stream_returns_none_for_a_local_song() {
        let mut local = make_song(Some(9), "local");
        local.source_url = Some("https://www.youtube.com/watch?v=abc".to_string());
        let files = vec![library_file(1, "C:/music/a.opus", Some("https://www.youtube.com/watch?v=abc"))];

        assert!(file_for_stream(&local, &files).is_none());
    }

    #[test]
    fn file_for_stream_ignores_files_without_id_or_path() {
        let stream = stream_song(STREAM_URL);
        let mut without_id = library_file(1, "C:/music/track.mp3", Some(STREAM_URL));
        without_id.id = None;
        let without_path = library_file(2, "", Some(STREAM_URL));
        let files = vec![without_id, without_path];

        assert!(file_for_stream(&stream, &files).is_none());
    }

    #[test]
    fn file_for_stream_returns_none_for_a_different_track() {
        let stream = stream_song("https://www.youtube.com/watch?v=other");
        let files = vec![library_file(1, "C:/music/a.opus", Some("https://www.youtube.com/watch?v=abc"))];

        assert!(file_for_stream(&stream, &files).is_none());
    }

    #[test]
    fn file_for_stream_returns_none_for_empty_files() {
        let stream = stream_song(STREAM_URL);

        assert!(file_for_stream(&stream, &[]).is_none());
    }

    #[test]
    fn order_push_on_an_empty_player_starts_at_cursor_zero() {
        let (mut order, mut cursor) = (None, None);
        order_push(&mut order, &mut cursor, 0);
        assert_eq!(order, Some(vec![0]));
        assert_eq!(cursor, Some(0));
    }

    #[test]
    fn order_push_appends_and_keeps_the_cursor() {
        let mut order = Some(vec![0, 1, 2]);
        let mut cursor = Some(1);
        order_push(&mut order, &mut cursor, 3);
        assert_eq!(order, Some(vec![0, 1, 2, 3]));
        assert_eq!(cursor, Some(1));
    }

    #[test]
    fn order_insert_next_goes_right_after_the_cursor() {
        let mut order = Some(vec![0, 1, 2]);
        let mut cursor = Some(0);
        order_insert_next(&mut order, &mut cursor, 3);
        assert_eq!(order, Some(vec![0, 3, 1, 2]));
        assert_eq!(cursor, Some(0));
    }

    #[test]
    fn order_insert_next_at_the_last_position_appends() {
        let mut order = Some(vec![0, 1]);
        let mut cursor = Some(1);
        order_insert_next(&mut order, &mut cursor, 2);
        assert_eq!(order, Some(vec![0, 1, 2]));
    }

    #[test]
    fn order_insert_next_without_cursor_or_order() {
        let mut order = Some(vec![0, 1]);
        let mut cursor = None;
        order_insert_next(&mut order, &mut cursor, 2);
        assert_eq!(order, Some(vec![0, 1, 2]));
        assert_eq!(cursor, Some(0));

        let (mut order, mut cursor) = (None, None);
        order_insert_next(&mut order, &mut cursor, 0);
        assert_eq!(order, Some(vec![0]));
        assert_eq!(cursor, Some(0));
    }

    #[test]
    fn inserting_a_batch_backwards_after_the_cursor_keeps_the_given_order() {
        // queue indices 0..3 are queued; indices 3, 4, 5 are the new songs A, B, C (in that order).
        let mut order = Some(vec![0, 1, 2]);
        let mut cursor = Some(0);
        for idx in [5usize, 4, 3] {
            order_insert_next(&mut order, &mut cursor, idx);
        }
        assert_eq!(order, Some(vec![0, 3, 4, 5, 1, 2]), "A B C play right after the current song");
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

    #[test]
    fn seek_clamp_fraction_nan_becomes_zero() {
        assert_eq!(clamp_fraction(f32::NAN), 0.0);
    }

    #[test]
    fn seek_clamp_fraction_negative_becomes_zero() {
        assert_eq!(clamp_fraction(-0.5), 0.0);
        assert_eq!(clamp_fraction(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn seek_clamp_fraction_above_one_becomes_one() {
        assert_eq!(clamp_fraction(1.5), 1.0);
        assert_eq!(clamp_fraction(f32::INFINITY), 1.0);
    }

    #[test]
    fn seek_clamp_fraction_inside_range_is_unchanged() {
        assert_eq!(clamp_fraction(0.25), 0.25);
        assert_eq!(clamp_fraction(0.999), 0.999);
    }

    #[test]
    fn seek_clamp_fraction_bounds_are_unchanged() {
        assert_eq!(clamp_fraction(0.0), 0.0);
        assert_eq!(clamp_fraction(1.0), 1.0);
    }

    #[test]
    fn seek_check_landed_exact_match_is_ok() {
        assert!(check_seek_landed(Duration::from_secs(120), Duration::from_secs(120)).is_ok());
    }

    #[test]
    fn seek_check_landed_within_tolerance_before_or_after_is_ok() {
        let target = Duration::from_secs(120);
        assert!(check_seek_landed(target, Duration::from_millis(121_500)).is_ok());
        assert!(check_seek_landed(target, Duration::from_millis(118_500)).is_ok());
    }

    #[test]
    fn seek_check_landed_just_outside_tolerance_is_err() {
        let target = Duration::from_secs(120);
        assert!(check_seek_landed(target, Duration::from_millis(121_501)).is_err());
        assert!(check_seek_landed(target, Duration::from_millis(118_499)).is_err());
    }

    #[test]
    fn seek_check_landed_seek_that_did_not_move_is_err_with_both_durations() {
        let err = check_seek_landed(Duration::from_secs(120), Duration::from_secs(5)).unwrap_err();
        assert!(err.contains("5s"), "message should contain landed position: {err}");
        assert!(err.contains("120s"), "message should contain target position: {err}");
    }

    #[test]
    fn seek_check_landed_far_after_target_is_err() {
        assert!(check_seek_landed(Duration::from_secs(10), Duration::from_secs(60)).is_err());
    }
}
