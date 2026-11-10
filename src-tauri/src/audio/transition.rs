//! Gapless transitions between songs, as pure decisions (the player applies them).
//!
//! Shortly before the current song ends, the next song of the queue is decoded and appended to the audio player,
//! so it starts without a gap. The transition is committed when the player reports that the current source has
//! actually finished (its source count drops), never by comparing the clock with a song length: lengths from the
//! database or from a platform are often slightly off, and a song of unknown length would end at once.
//!
//! The queue can change between appending the next song and the moment it starts (the user removes or moves songs,
//! changes the repeat mode or shuffles). [`resolve_transition`] decides at that moment whether the appended audio is
//! still the right song.

use crate::types::Song;
use std::time::Duration;

/// How long before the end of the current song the next one is appended.
pub const BUFFER_LOOKAHEAD: Duration = Duration::from_secs(5);

/// Identity of a queued song, to check that the appended audio and the queue still agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SongKey {
    Library(i64),
    Stream(String),
    File(String),
}

pub fn song_key(song: &Song) -> SongKey {
    match (song.id, song.source_url.as_deref()) {
        (Some(id), _) => SongKey::Library(id),
        (None, Some(url)) if song.path.is_empty() && !url.is_empty() => SongKey::Stream(url.to_string()),
        _ => SongKey::File(song.path.clone()),
    }
}

/// The song appended for a gapless transition, waiting for the current source to finish.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingNext {
    /// Play-order position the song had when it was appended.
    pub cursor: usize,
    pub key: SongKey,
    /// Length reported by its decoder (or the platform), when known.
    pub duration: Option<Duration>,
    /// Number of sources in the audio player right after appending: one less means the current song finished.
    pub sources_at_append: usize,
}

impl PendingNext {
    /// True when the source that was playing before the append has finished.
    pub fn started(&self, sources_now: usize) -> bool {
        sources_now < self.sources_at_append
    }
}

/// What to do once the appended song has started playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// The appended song is the one the queue expects: keep it playing and make it the current song.
    Adopt(usize),
    /// The queue changed meanwhile: replace the appended audio with the song the queue expects now.
    Restart(usize),
    /// Nothing should play after the previous song any more (end of the queue).
    Stop,
}

/// Decides the transition from the appended song and the song the queue expects next (its position and key).
pub fn resolve_transition(appended: &SongKey, expected: Option<(usize, &SongKey)>) -> Transition {
    match expected {
        Some((cursor, key)) if key == appended => Transition::Adopt(cursor),
        Some((cursor, _)) => Transition::Restart(cursor),
        None => Transition::Stop,
    }
}

/// True when the current song is close enough to its end to append the next one. A song of unknown length is
/// never buffered ahead: its end is detected when the audio runs dry.
pub fn should_buffer(duration: Option<Duration>, position: Duration) -> bool {
    duration
        .filter(|d| !d.is_zero())
        .is_some_and(|d| d.saturating_sub(position) <= BUFFER_LOOKAHEAD)
}

/// Length of the current song: the decoder's, else the stored one; `None` when neither is known.
pub fn effective_duration(decoded: Option<Duration>, stored_ms: u64) -> Option<Duration> {
    decoded
        .filter(|d| !d.is_zero())
        .or_else(|| Some(Duration::from_millis(stored_ms)).filter(|d| !d.is_zero()))
}

/// Playback progress in 0..=1 for the UI; 0 when the length is unknown.
pub fn progress_fraction(position: Duration, duration: Option<Duration>) -> f32 {
    match duration.filter(|d| !d.is_zero()) {
        Some(d) => (position.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Platform, QualityTier};

    fn song(id: Option<i64>, path: &str, url: Option<&str>) -> Song {
        Song {
            id,
            path: path.into(),
            title: "t".into(),
            artists: vec!["a".into()],
            album_artist: "a".into(),
            album: "b".into(),
            duration_ms: 1000,
            track_number: None,
            genre: None,
            release_year: None,
            cover_url: None,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source: Platform::Local,
            source_url: url.map(str::to_string),
            source_item_id: None,
            canonical_track_slug: String::new(),
            canonical_album_slug: String::new(),
            quality_tier: QualityTier::Standard,
        }
    }

    #[test]
    fn song_key_prefers_the_library_id() {
        assert_eq!(song_key(&song(Some(4), "/a.mp3", Some("https://x"))), SongKey::Library(4));
    }

    #[test]
    fn song_key_of_a_stream_is_its_url() {
        assert_eq!(song_key(&song(None, "", Some("https://x"))), SongKey::Stream("https://x".into()));
    }

    #[test]
    fn song_key_without_id_or_stream_url_is_the_path() {
        assert_eq!(song_key(&song(None, "/a.mp3", None)), SongKey::File("/a.mp3".into()));
        assert_eq!(song_key(&song(None, "/a.mp3", Some("https://x"))), SongKey::File("/a.mp3".into()));
        assert_eq!(song_key(&song(None, "", Some(""))), SongKey::File(String::new()));
    }

    #[test]
    fn pending_next_starts_when_a_source_finished() {
        let pending = PendingNext { cursor: 1, key: SongKey::Library(1), duration: None, sources_at_append: 2 };
        assert!(!pending.started(2));
        assert!(pending.started(1));
        assert!(pending.started(0));
    }

    #[test]
    fn transition_adopts_the_appended_song_when_the_queue_still_expects_it() {
        let key = SongKey::Library(7);
        assert_eq!(resolve_transition(&key, Some((3, &SongKey::Library(7)))), Transition::Adopt(3));
    }

    #[test]
    fn transition_follows_the_new_position_of_the_same_song() {
        // the song was moved: it is still next, at another position of the play order
        let key = SongKey::Library(7);
        assert_eq!(resolve_transition(&key, Some((5, &SongKey::Library(7)))), Transition::Adopt(5));
    }

    #[test]
    fn transition_restarts_when_another_song_is_next_now() {
        let key = SongKey::Library(7);
        assert_eq!(resolve_transition(&key, Some((3, &SongKey::Library(8)))), Transition::Restart(3));
        let stream = SongKey::Stream("https://a".into());
        assert_eq!(
            resolve_transition(&stream, Some((0, &SongKey::Stream("https://b".into())))),
            Transition::Restart(0)
        );
    }

    #[test]
    fn transition_stops_when_nothing_is_next_any_more() {
        assert_eq!(resolve_transition(&SongKey::Library(1), None), Transition::Stop);
    }

    #[test]
    fn should_buffer_only_inside_the_lookahead_of_a_known_length() {
        let len = Duration::from_secs(200);
        assert!(!should_buffer(Some(len), Duration::from_secs(100)));
        assert!(!should_buffer(Some(len), len - BUFFER_LOOKAHEAD - Duration::from_millis(1)));
        assert!(should_buffer(Some(len), len - BUFFER_LOOKAHEAD));
        assert!(should_buffer(Some(len), len));
        assert!(should_buffer(Some(len), len + Duration::from_secs(3)));
    }

    #[test]
    fn should_buffer_never_for_an_unknown_or_zero_length() {
        assert!(!should_buffer(None, Duration::from_secs(1000)));
        assert!(!should_buffer(Some(Duration::ZERO), Duration::ZERO));
    }

    #[test]
    fn should_buffer_a_song_shorter_than_the_lookahead_from_the_start() {
        assert!(should_buffer(Some(Duration::from_secs(3)), Duration::ZERO));
    }

    #[test]
    fn effective_duration_prefers_the_decoder() {
        let decoded = Some(Duration::from_millis(1500));
        assert_eq!(effective_duration(decoded, 9000), decoded);
        assert_eq!(effective_duration(None, 9000), Some(Duration::from_millis(9000)));
        assert_eq!(effective_duration(Some(Duration::ZERO), 9000), Some(Duration::from_millis(9000)));
        assert_eq!(effective_duration(None, 0), None);
        assert_eq!(effective_duration(Some(Duration::ZERO), 0), None);
    }

    #[test]
    fn progress_fraction_is_bounded_and_zero_without_a_length() {
        let len = Some(Duration::from_secs(100));
        assert_eq!(progress_fraction(Duration::from_secs(25), len), 0.25);
        assert_eq!(progress_fraction(Duration::from_secs(150), len), 1.0);
        assert_eq!(progress_fraction(Duration::from_secs(5), None), 0.0);
        assert_eq!(progress_fraction(Duration::from_secs(5), Some(Duration::ZERO)), 0.0);
    }
}
