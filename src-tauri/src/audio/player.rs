use crate::audio::output::AudioOutput;
use crate::audio::queue_ops;
use crate::audio::transition::{
    effective_duration, progress_fraction, resolve_transition, should_buffer, song_key, PendingNext, SongKey,
    Transition,
};
use crate::discord_presence::DiscordRpcClient;
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
use rodio::{Decoder, Player, Source};
use serde::{Deserialize, Serialize};
use souvlaki::MediaMetadata;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use std::{fs::File, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

/// How often the background watchers look at the player.
const TRANSITION_TICK: Duration = Duration::from_millis(100);
const DEVICE_TICK: Duration = Duration::from_millis(500);
/// Files above this size are not copied into memory for a tag edit (the decoder is released instead).
pub const MAX_IN_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
/// Event sent to the UI when a song cannot be played (payload: [`PlaybackError`]).
pub const PLAYBACK_ERROR_EVENT: &str = "playback-error";

/// Locks the player. A panic while the lock was held must not disable playback until the app restarts, so a
/// poisoned lock is recovered: every method leaves the player in a consistent state between statements that can
/// panic, and the next command overwrites whatever was half done.
pub fn lock_player(state: &Mutex<AudioPlayer>) -> MutexGuard<'_, AudioPlayer> {
    state.lock().unwrap_or_else(|poisoned| {
        eprintln!("[audio] recovering the player after a panic in another thread");
        state.clear_poison();
        poisoned.into_inner()
    })
}

/// A volume from the UI forced into 0..=1 (more than 1 amplifies and can damage hearing and speakers).
pub fn clamp_volume(volume: f32) -> f32 {
    if volume.is_nan() { 0.0 } else { volume.clamp(0.0, 1.0) }
}

/// URI of a cover image for the OS media controls. A local file is passed as `file://<path>`: souvlaki strips the
/// prefix and opens the path itself on Windows (so the path keeps its native form there), and on Linux an absolute
/// path gives the standard `file:///...`. Web urls are passed unchanged.
pub fn cover_uri(path: &str) -> String {
    if is_web_url(path) {
        return path.to_string();
    }
    #[cfg(target_os = "windows")]
    let path = path.replace('/', "\\");
    format!("file://{path}")
}

/// True for an http(s) url.
pub fn is_web_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Why a song could not be played, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct PlaybackError {
    pub title: String,
    pub message: String,
}

/// One update of the Discord presence, applied in order by the presence thread.
struct DiscordUpdate {
    song: Song,
    duration_ms: i64,
    position_ms: i64,
    paused: bool,
}

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
    let off = landed.abs_diff(target);
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
    if is_stream_song(song) || file_exists || song.source_url.as_deref().is_none_or(str::is_empty) {
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
        if gone.source_url.as_deref().is_none_or(str::is_empty) {
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

/// Appends queue index `new_idx` at the end of the play order. An empty player starts at cursor 0; a player with
/// a queue but no current song points the cursor at the first added song.
fn order_push(order: &mut Option<Vec<usize>>, cursor: &mut Option<usize>, new_idx: usize) {
    if let Some(order) = order {
        order.push(new_idx);
        if cursor.is_none() {
            *cursor = Some(order.len() - 1);
        }
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
            // nothing is current: the added song is what plays next, so the cursor points at it
            order.push(new_idx);
            *cursor = Some(order.len() - 1);
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
    output: AudioOutput,
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
    /// The next song, appended to `player` for a gapless transition (see `audio::transition`).
    pending_next: Option<PendingNext>,
    /// Bumped whenever the audio in `player` is replaced; a background decode only applies to the audio it was
    /// started for.
    audio_generation: u64,
    /// A file of the next song is being decoded in the background.
    buffering: bool,
    /// The last song a gapless append was tried for (generation and key): a file that cannot be decoded is not
    /// tried again on every tick; the transition then happens when the current song ends.
    buffer_attempt: Option<(u64, SongKey)>,
    /// Discord presence updates, applied in order by a dedicated thread.
    discord: mpsc::Sender<DiscordUpdate>,
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

/// A next song to decode outside the player lock (see `AudioPlayer::tick`).
struct BufferJob {
    generation: u64,
    key: SongKey,
    path: String,
}

impl AudioPlayer {
    pub fn new(media_controls: MediaControls, handle: AppHandle) -> Self {
        let output = AudioOutput::open_default_or_null();
        let player = Player::connect_new(output.mixer());
        // Without a real device there is no device to compare with: the watcher switches once one appears.
        let current_device_id = if output.is_null() { None } else { Self::get_default_device_id().ok() };
        let discord = Self::spawn_discord_worker(handle.clone());

        Self::spawn_device_watcher(handle.clone());
        Self::spawn_transition_watcher(handle.clone());

        Self {
            output,
            player,
            duration: None,
            seek_offset: Duration::ZERO,
            media_controls,
            current_song: None,
            handle,
            queue: Vec::new(),
            current_device_id,
            shuffle_mode: false,
            repeat_mode: RepeatMode::Off,
            play_order: None,
            cursor: None,
            stream_loading: None,
            stream_backing_id: None,
            stream_generation: 0,
            prepared: None,
            preparing: None,
            pending_next: None,
            audio_generation: 0,
            buffering: false,
            buffer_attempt: None,
            discord,
        }
    }

    /// Switches to the system's default output device whenever it changes (or appears).
    fn spawn_device_watcher(handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(DEVICE_TICK).await;
                // queried without the player lock: enumerating devices can be slow
                let Ok(default_id) = Self::get_default_device_id() else {
                    continue;
                };
                let state = handle.state::<Mutex<AudioPlayer>>();
                let mut player = lock_player(&state);
                if player.current_device_id.as_ref() != Some(&default_id) {
                    println!(
                        "[audio] default output device changed from {:?} to {default_id:?}",
                        player.current_device_id
                    );
                    player.reconnect_default_device(default_id);
                }
            }
        });
    }

    /// Drives gapless transitions (see `audio::transition`). Decoding the next file happens outside the player lock,
    /// so commands are never blocked by disk or decoder work.
    fn spawn_transition_watcher(handle: AppHandle) {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(TRANSITION_TICK).await;
                let state = handle.state::<Mutex<AudioPlayer>>();
                let job = lock_player(&state).tick();
                let Some(job) = job else { continue };
                let path = job.path.clone();
                let decoded = tauri::async_runtime::spawn_blocking(move || {
                    Self::decode_source(&path).map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("decoder task failed: {e}")));
                lock_player(&state).finish_buffering(job, decoded);
            }
        });
    }

    /// One step of the transition watcher. Commits a transition whose appended song started, moves on when the
    /// audio ran dry, or appends the next song shortly before the end. Returns a decode job for the next file, which
    /// the caller runs without the lock and hands back to `finish_buffering`.
    fn tick(&mut self) -> Option<BufferJob> {
        if self.is_paused() || self.stream_loading.is_some() || self.current_song.is_none() {
            return None;
        }
        if let Some(pending) = &self.pending_next {
            if pending.started(self.player.len()) {
                self.commit_transition();
            }
            return None;
        }
        if self.player.empty() {
            // the song ended and nothing was appended (unknown length, a stream that was not ready, a file that
            // could not be decoded ahead of time, or a stream that ended early)
            self.advance_after_end();
            return None;
        }
        if self.buffering || !should_buffer(self.duration, self.current_position()) {
            return None;
        }

        let cursor = self.get_next_cursor()?;
        self.stream_missing_file_at(cursor);
        let song = self.song_at(cursor)?;
        let key = song_key(&song);
        if self.buffer_attempt.as_ref() == Some(&(self.audio_generation, key.clone())) {
            return None;
        }
        if is_stream_song(&song) {
            // only an already opened stream can be appended; otherwise it opens when the current song ends
            let url = song.source_url.clone().unwrap_or_default();
            if let Some((source, duration)) = self.take_prepared(&url) {
                let duration = effective_duration(source.total_duration().or(duration), song.duration_ms);
                self.append_pending(cursor, key, source, duration);
            }
            return None;
        }
        self.buffering = true;
        self.buffer_attempt = Some((self.audio_generation, key.clone()));
        Some(BufferJob { generation: self.audio_generation, key, path: song.path })
    }

    /// Appends a decoded next file, unless the audio or the queue changed while it was decoded.
    fn finish_buffering(&mut self, job: BufferJob, decoded: Result<BoxedSource, String>) {
        if job.generation != self.audio_generation {
            return; // the audio was replaced meanwhile; `buffering` was reset with it
        }
        self.buffering = false;
        if self.pending_next.is_some() {
            return;
        }
        let Some(cursor) = self.get_next_cursor() else { return };
        let Some(song) = self.song_at(cursor).filter(|s| song_key(s) == job.key) else {
            return; // another song is next now; the next tick buffers that one
        };
        match decoded {
            Ok(source) => {
                let duration = effective_duration(source.total_duration(), song.duration_ms);
                self.append_pending(cursor, job.key, source, duration);
            }
            Err(e) => eprintln!("[audio] cannot decode the next song {:?} ahead of time: {e}", song.title),
        }
    }

    fn append_pending(&mut self, cursor: usize, key: SongKey, source: BoxedSource, duration: Option<Duration>) {
        self.player.append(source);
        self.pending_next = Some(PendingNext {
            cursor,
            key,
            duration,
            sources_at_append: self.player.len(),
        });
    }

    /// The appended song started: make it the current song, or correct course when the queue changed since.
    fn commit_transition(&mut self) {
        let Some(pending) = self.pending_next.take() else { return };
        let expected = self.get_next_cursor().and_then(|c| self.song_at(c).map(|s| (c, song_key(&s))));
        match resolve_transition(&pending.key, expected.as_ref().map(|(c, k)| (*c, k))) {
            Transition::Adopt(cursor) => {
                let Some(song) = self.song_at(cursor) else { return self.stop() };
                self.cursor = Some(cursor);
                self.stream_backing_id = None;
                self.duration = pending.duration.or_else(|| effective_duration(None, song.duration_ms));
                self.seek_offset = Duration::ZERO;
                self.current_song = Some(song);
                self.adopt_stream_duration();
                self.media_playing();
                self.update_current_metadata();
                self.emit_state();
                self.update_discord_song(Some(0));
                self.prepare_next();
            }
            Transition::Restart(cursor) => self.play_at(cursor),
            Transition::Stop => self.stop(),
        }
    }

    /// The current song ended by itself: plays what the queue has next, or stops at the end of the queue.
    fn advance_after_end(&mut self) {
        match self.get_next_cursor() {
            Some(cursor) => self.play_at(cursor),
            None => self.stop(),
        }
    }

    /// Moves the cursor to play-order position `cursor` and plays that song.
    fn play_at(&mut self, cursor: usize) {
        self.stream_missing_file_at(cursor);
        match self.song_at(cursor) {
            Some(song) => {
                self.cursor = Some(cursor);
                self.play(song);
            }
            None => self.stop(),
        }
    }

    /// Empties the audio player (it pauses as well). Everything appended or being decoded for it is dropped.
    fn reset_audio(&mut self) {
        self.player.clear();
        self.forget_appended();
    }

    /// The audio in `player` was replaced: a pending transition and an in-flight decode no longer apply.
    fn forget_appended(&mut self) {
        self.pending_next = None;
        self.buffering = false;
        self.buffer_attempt = None;
        self.audio_generation = self.audio_generation.wrapping_add(1);
    }

    /// Position in the current song (the audio player's position restarts at 0 when its source is replaced).
    fn current_position(&self) -> Duration {
        self.seek_offset + self.player.get_pos()
    }

    fn media_playing(&mut self) {
        let position = self.current_position();
        self.media_controls.set_playing(position);
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

    /// Plays `song` from the start as the current song. The cursor must already point at it. A file that cannot be
    /// decoded leaves the player stopped on that song (so the UI and the queue agree) and reports the error.
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
        self.reset_audio();
        self.seek_offset = Duration::ZERO;

        let source = match Self::decode_source(&song.path) {
            Ok(source) => source,
            Err(e) => return self.fail_current(song, &e.to_string()),
        };
        self.duration = effective_duration(source.total_duration(), song.duration_ms);
        self.player.append(source);
        self.player.play();
        self.current_song = Some(song);

        self.update_current_metadata();
        self.media_playing();
        self.emit_state();
        self.update_discord_song(Some(0));
        self.prepare_next();
    }

    /// `song` (the current song) cannot be played: the player stops on it and the UI is told why.
    fn fail_current(&mut self, song: Song, message: &str) {
        eprintln!("[audio] cannot play {:?} ({}): {message}", song.title, song.path);
        let error = PlaybackError { title: song.title.clone(), message: message.to_string() };
        self.current_song = Some(song);
        self.duration = None;
        self.seek_offset = Duration::ZERO;
        self.player.pause();
        self.media_controls.set_stopped();
        self.update_current_metadata();
        self.emit_state();
        let _ = self.handle.emit(PLAYBACK_ERROR_EVENT, error);
    }

    fn decode_source(path_str: &str) -> Result<BoxedSource, Box<dyn std::error::Error>> {
        let path = Path::new(path_str);

        // Opus files use the project's own decoder; rodio's covers MP3, WAV, FLAC, AAC and the rest.
        if path.extension().and_then(|ext| ext.to_str()) == Some("opus") {
            let opus_source = OpusSource::open(path)?;
            return Ok(Box::new(opus_source));
        }

        let file = File::open(path)?;
        let decoder = Decoder::try_from(file)?;
        Ok(Box::new(decoder))
    }

    pub fn pause(&mut self) {
        self.player.pause();
        let position = self.current_position();
        self.media_controls.set_paused(position);
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song(None);
    }

    pub fn resume(&mut self) {
        if self.player.empty() {
            if self.stream_loading.is_some() {
                return;
            }
            // nothing is loaded: the current song, or the song the cursor points at (a queue without current song)
            if let Some(song) = self.current_song.clone() {
                return self.play(song);
            }
            if let Some(cursor) = self.cursor {
                return self.play_at(cursor);
            }
            return;
        }
        self.player.play();
        self.media_playing();
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song(None);
    }

    /// Stops playback: the audio is released (no file stays open), the current song and the queue are kept.
    pub fn stop(&mut self) {
        self.reset_audio();
        self.seek_offset = Duration::ZERO;
        self.duration = None;
        self.media_controls.set_stopped();
        self.emit_state();
        self.update_current_metadata();
        self.update_discord_song(None);
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
    /// Without a current song both modes append, and the cursor points at the first added song.
    pub fn add_songs_to_queue(&mut self, songs: Vec<Song>, play_next: bool) {
        if songs.is_empty() {
            return;
        }
        if play_next && self.cursor.is_some() {
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
            match removal.cursor {
                Some(cursor) => return self.play_at(cursor),
                None => {
                    self.stop();
                    self.current_song = None;
                    self.cursor = None;
                }
            }
        } else {
            self.prepare_next();
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
        self.prepare_next();
        self.emit_state();
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

    /// Replaces the queue with `songs` and plays the one at `current_index` (clamped into the queue). With shuffle
    /// on, the chosen song plays first and the rest is shuffled. An empty list stops playback and clears the queue.
    pub fn start_playback(&mut self, songs: Vec<Song>, current_index: usize) {
        let Some(start) = queue_ops::clamp_start(songs.len(), current_index) else {
            self.stop();
            self.queue.clear();
            self.play_order = None;
            self.cursor = None;
            self.current_song = None;
            self.emit_state();
            return;
        };
        self.play_order = Some((0..songs.len()).collect());
        self.queue = songs;
        self.cursor = Some(start);
        if self.shuffle_mode {
            self.apply_shuffle_to_queue();
        }
        match self.cursor {
            Some(cursor) => self.play_at(cursor),
            None => self.stop(),
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

    /// Position of the song that plays when the current one ends (see `queue_ops::next_on_end`).
    fn get_next_cursor(&self) -> Option<usize> {
        let len = self.play_order.as_ref()?.len();
        queue_ops::next_on_end(len, self.cursor, self.repeat_mode)
    }

    pub fn toggle_shuffle(&mut self) {
        if self.shuffle_mode {
            if let Some(order) = self.play_order.as_deref() {
                let (order, cursor) = queue_ops::unshuffle(order, self.cursor);
                self.play_order = Some(order);
                self.cursor = cursor;
            }
            self.shuffle_mode = false;
        } else {
            self.shuffle_mode = true;
            self.apply_shuffle_to_queue();
        }
        self.prepare_next();
        self.emit_state();
    }

    pub fn set_repeat_mode(&mut self, repeat_mode: RepeatMode) {
        self.repeat_mode = repeat_mode;
        // the song after the current one may be another one now
        self.prepare_next();
        self.emit_state();
    }

    fn apply_shuffle_to_queue(&mut self) {
        if let Some(order) = self.play_order.as_deref() {
            let (order, cursor) =
                queue_ops::shuffle_keeping_current(order, self.cursor, |rest| rest.shuffle(&mut rng()));
            self.play_order = Some(order);
            // without a current song the cursor stays where it was (none)
            if self.cursor.is_some() {
                self.cursor = cursor;
            }
        }
    }

    fn update_current_metadata(&mut self) {
        let Some(song) = &self.current_song else {
            return;
        };
        let cover = song.cover_url.as_deref().map(cover_uri);
        let display_artists = song.artists.join(", ");
        let duration = self.duration.or_else(|| effective_duration(None, song.duration_ms));

        self.media_controls.update_metadata(MediaMetadata {
            title: Some(&song.title),
            artist: Some(&display_artists),
            album: Some(&song.album),
            duration,
            cover_url: cover.as_deref(),
        });
    }

    /// Skips to the next song of the queue (repeat-one moves on as well; repeat-all wraps around). At the end of the
    /// queue without repeat, playback stops on the last song.
    pub fn next(&mut self) {
        let len = self.play_order.as_ref().map_or(0, Vec::len);
        match queue_ops::next_on_skip(len, self.cursor, self.repeat_mode) {
            Some(cursor) => self.play_at(cursor),
            None if self.current_song.is_some() => self.stop(),
            None => {}
        }
    }

    /// Restarts the current song after its first 5 seconds; otherwise goes to the previous song (the first song
    /// restarts).
    pub fn previous(&mut self) {
        let Some(cursor) = self.cursor else {
            return;
        };
        let target = if self.current_position() > Duration::from_secs(5) {
            cursor
        } else {
            cursor.saturating_sub(1)
        };
        self.play_at(target);
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
            println!("[seek] target {target:?} is within 1 s of the end ({duration:?}): the song counts as ended");
            self.advance_after_end();
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

    /// Progress of the current song in 0..=1 (0 when its length is unknown).
    pub fn position(&self) -> f32 {
        progress_fraction(self.current_position(), self.duration)
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

    /// Path of the current song's file when `id` is the current song (a library file, not a stream).
    pub fn current_file_of(&self, id: i64) -> Option<String> {
        self.current_song
            .as_ref()
            .filter(|s| s.id == Some(id) && !s.path.is_empty() && !self.current_is_stream_audio())
            .map(|s| s.path.clone())
    }

    /// If `id` is the current song, switches playback to `bytes` (an in-memory copy of its file, read by the caller
    /// without holding the player lock) at (almost) the same position, so the file on disk is no longer open and
    /// can be rewritten with no audible gap. Does nothing for other songs. On error playback is left untouched.
    pub fn move_song_to_memory(&mut self, id: i64, bytes: Vec<u8>) -> Result<(), String> {
        let Some(song) = self
            .current_song
            .clone()
            .filter(|s| s.id == Some(id))
        else {
            return Ok(());
        };

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
        self.forget_appended();
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
        let pos = self.current_position();
        let paused = self.is_paused();
        self.reset_audio();
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
        let state = PlayerState {
            current_song: self.current_song.clone(),
            is_playing: !self.is_paused(),
            progress: self.position(),
            volume: self.player.volume(),
            shuffle_mode: self.shuffle_mode,
            repeat_mode: self.repeat_mode,
            queue: self.get_ordered_queue(),
            queue_cursor: self.cursor,
            source_index: source_index_at(self.play_order.as_deref(), self.cursor),
            streaming: self.current_song.as_ref().is_some_and(is_stream_song),
            stream_loading: self.stream_loading.is_some(),
        };
        self.handle.emit("player-state", state).ok();
    }

    /// Starts the thread that applies Discord presence updates. Updates are applied one at a time in the order they
    /// were made, and only the newest of a burst is sent (Discord rate-limits presence changes anyway).
    fn spawn_discord_worker(handle: AppHandle) -> mpsc::Sender<DiscordUpdate> {
        let (tx, rx) = mpsc::channel::<DiscordUpdate>();
        let spawned = std::thread::Builder::new().name("discord-presence".into()).spawn(move || {
            while let Ok(mut update) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    update = newer;
                }
                Self::apply_discord_update(&handle, update);
            }
        });
        if let Err(e) = spawned {
            eprintln!("[discord] cannot start the presence thread: {e}");
        }
        tx
    }

    fn apply_discord_update(handle: &AppHandle, update: DiscordUpdate) {
        let state = handle.state::<Mutex<Option<DiscordRpcClient>>>();
        let mut client = state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if client.is_none() {
            match DiscordRpcClient::new(handle.clone()) {
                Ok(created) => *client = Some(created),
                Err(e) => {
                    eprintln!("[discord] cannot create the client: {e}");
                    return;
                }
            }
        }
        let Some(discord) = client.as_mut() else { return };
        if !discord.is_connected() && discord.reconnect().is_err() {
            return;
        }
        if let Err(e) = discord.update_song(&update.song, update.duration_ms, update.position_ms, update.paused) {
            eprintln!("[discord] update failed, disconnecting: {e}");
            discord.is_connected = false;
        }
    }

    /// Sends the current song to the Discord presence. `force_position_ms` overrides the measured position (0 right
    /// after a song starts, the target after a seek).
    fn update_discord_song(&self, force_position_ms: Option<i64>) {
        let Some(song) = self.current_song.clone() else {
            return;
        };
        let position_ms =
            force_position_ms.unwrap_or_else(|| i64::try_from(self.current_position().as_millis()).unwrap_or(0));
        let duration_ms = self
            .duration
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        let update = DiscordUpdate { song, duration_ms, position_ms, paused: self.is_paused() };
        // the thread only stops with the app
        let _ = self.discord.send(update);
    }

    /// A cover found for the presence of song `song_id`: shown from now on if it is still the current song.
    /// Only web urls are accepted.
    pub fn apply_external_cover(&mut self, song_id: i64, url: String) {
        if !is_web_url(&url) {
            return;
        }
        let Some(song) = self.current_song.as_mut().filter(|s| s.id == Some(song_id)) else {
            return;
        };
        song.external_cover_url = Some(url);
        self.update_discord_song(None);
    }

    /// Moves playback to the new default output device: the current song continues at the same position, volume
    /// and paused state. A device that cannot be opened is remembered, so it is not retried every tick; the next
    /// change of the default device tries again.
    fn reconnect_default_device(&mut self, device_id: DeviceId) {
        self.current_device_id = Some(device_id);
        let output = match AudioOutput::open_default() {
            Ok(output) => output,
            Err(e) => {
                eprintln!("[audio] keeping the previous output: {e}");
                return;
            }
        };

        let was_paused = self.is_paused();
        let volume = self.player.volume();
        let position = self.current_position();
        let song = self.current_song.clone();

        // the old player goes before its output: dropping it stops whatever it still plays
        self.player = Player::connect_new(output.mixer());
        self.output = output;
        self.player.set_volume(volume);
        self.forget_appended();

        match song {
            // a stream that is opening ends up in the new player when it is ready
            _ if self.stream_loading.is_some() => {}
            None => {}
            // a stream is reopened from the network (from the start)
            Some(song) if is_stream_song(&song) => {
                self.play(song);
                if was_paused {
                    self.pause();
                }
            }
            Some(song) => self.resume_file_at(song, position, was_paused),
        }
        self.emit_state();
    }

    /// Plays the file of `song` from `position` (used after the output device changed).
    fn resume_file_at(&mut self, song: Song, position: Duration, paused: bool) {
        let mut source = match Self::decode_source(&song.path) {
            Ok(source) => source,
            Err(e) => return self.fail_current(song, &e.to_string()),
        };
        self.seek_offset = Duration::ZERO;
        if !position.is_zero() && source.try_seek(position).is_ok() {
            self.seek_offset = position;
        }
        self.player.append(source);
        if paused {
            self.player.pause();
        } else {
            self.player.play();
        }
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
        self.reset_audio();
        self.seek_offset = Duration::ZERO;
        self.duration = effective_duration(None, song.duration_ms);
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
        self.duration = effective_duration(source.total_duration().or(duration), song.duration_ms);
        self.seek_offset = Duration::ZERO;
        self.reset_audio();
        self.player.append(source);
        self.player.play();
        self.current_song = Some(song);
        self.adopt_stream_duration();

        self.update_current_metadata();
        self.media_playing();
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

    /// Sets the volume (clamped to 0..=1: the UI can never amplify).
    pub fn set_volume(&mut self, fraction: f32) {
        self.player.set_volume(clamp_volume(fraction));
    }

    pub fn get_volume(&self) -> f32 {
        self.player.volume()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_song_edit, check_seek_landed, clamp_fraction, clamp_volume, cover_uri, file_for_stream, is_stream_song,
        is_web_url, order_insert_next, order_push, source_index_at, stream_fallback, streamify_deleted,
        swap_streams_for_files, AudioPlayer,
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
        // nothing was current: the cursor points at the song just added (it plays next)
        assert_eq!(cursor, Some(2));

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

    #[test]
    fn clamp_volume_nan_becomes_zero() {
        assert_eq!(clamp_volume(f32::NAN), 0.0);
    }

    #[test]
    fn clamp_volume_negative_becomes_zero() {
        assert_eq!(clamp_volume(-0.5), 0.0);
    }

    #[test]
    fn clamp_volume_above_one_becomes_one() {
        assert_eq!(clamp_volume(50.0), 1.0);
    }

    #[test]
    fn clamp_volume_inside_range_is_unchanged() {
        assert_eq!(clamp_volume(0.3), 0.3);
    }

    #[test]
    fn clamp_volume_bounds_are_unchanged() {
        assert_eq!(clamp_volume(0.0), 0.0);
        assert_eq!(clamp_volume(1.0), 1.0);
    }

    #[test]
    fn is_web_url_accepts_http_and_https() {
        assert!(is_web_url("https://example.com/cover.jpg"));
        assert!(is_web_url("http://example.com/cover.jpg"));
    }

    #[test]
    fn is_web_url_rejects_other_schemes_paths_and_empty() {
        assert!(!is_web_url("file://x"));
        assert!(!is_web_url("/covers/a.jpg"));
        assert!(!is_web_url("C:\\a.jpg"));
        assert!(!is_web_url(""));
    }

    #[test]
    fn cover_uri_web_url_is_unchanged() {
        assert_eq!(cover_uri("https://example.com/cover.jpg"), "https://example.com/cover.jpg");
    }

    #[test]
    fn cover_uri_local_path_gets_file_prefix() {
        assert_eq!(cover_uri("cover.jpg"), "file://cover.jpg");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cover_uri_windows_path_uses_backslashes() {
        assert_eq!(cover_uri("C:/covers/a.jpg"), "file://C:\\covers\\a.jpg");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn cover_uri_unix_absolute_path_gives_standard_file_uri() {
        assert_eq!(cover_uri("/covers/a.jpg"), "file:///covers/a.jpg");
    }

    #[test]
    fn order_push_with_order_but_no_cursor_points_cursor_at_pushed_song() {
        let mut order = Some(vec![0, 1]);
        let mut cursor = None;
        order_push(&mut order, &mut cursor, 2);
        assert_eq!(order, Some(vec![0, 1, 2]));
        assert_eq!(cursor, Some(2));
    }

    #[test]
    fn order_push_second_push_keeps_the_cursor() {
        let mut order = Some(vec![0, 1]);
        let mut cursor = None;
        order_push(&mut order, &mut cursor, 2);
        order_push(&mut order, &mut cursor, 3);
        assert_eq!(order, Some(vec![0, 1, 2, 3]));
        assert_eq!(cursor, Some(2));
    }
}
