use std::time::{Duration, Instant};

use crate::types::Song;

/// Minimum interval between Discord IPC updates and between retry attempts when out of sync.
pub const DISCORD_SYNC_INTERVAL: Duration = Duration::from_secs(5);

/// Tolerance for timestamp drift before considering the playback position changed (e.g. seek/stall).
const TIMESTAMP_DRIFT_TOLERANCE_MS: i64 = 1_500;

/// Desired Discord Rich Presence state derived from playback events.
#[derive(Debug, Clone)]
pub struct DesiredPresence {
    pub song: Song,
    pub duration_ms: i64,
    pub start_epoch_ms: i64,
    pub end_epoch_ms: i64,
    pub paused: bool,
}

impl DesiredPresence {
    pub fn from_playback(
        song: Song,
        duration_ms: i64,
        position_ms: i64,
        paused: bool,
        now_epoch_ms: i64,
    ) -> Self {
        let clamped_pos = position_ms.max(0);
        let clamped_dur = duration_ms.max(0);
        let start_epoch_ms = now_epoch_ms - clamped_pos;
        let end_epoch_ms = start_epoch_ms + clamped_dur;
        Self {
            song,
            duration_ms: clamped_dur,
            start_epoch_ms,
            end_epoch_ms,
            paused,
        }
    }
}

/// Comparable representation of what was last pushed to Discord IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushedPresence {
    Cleared,
    Playing {
        song_id: Option<i64>,
        title: String,
        artists: Vec<String>,
        album: String,
        cover_url: Option<String>,
        source_url: Option<String>,
        duration_ms: i64,
        start_epoch_ms: i64,
    },
}

impl PushedPresence {
    pub fn from_desired(desired: &DesiredPresence, resolved_cover_url: Option<String>) -> Self {
        if desired.paused {
            return Self::Cleared;
        }
        Self::Playing {
            song_id: desired.song.id,
            title: desired.song.title.clone(),
            artists: desired.song.artists.clone(),
            album: desired.song.album.clone(),
            cover_url: resolved_cover_url.or_else(|| initial_cover_url(&desired.song)),
            source_url: desired.song.source_url.clone(),
            duration_ms: desired.duration_ms,
            start_epoch_ms: desired.start_epoch_ms,
        }
    }

    pub fn matches(&self, desired: &DesiredPresence) -> bool {
        match (self, desired.paused) {
            (Self::Cleared, true) => true,
            (Self::Cleared, false) | (Self::Playing { .. }, true) => false,
            (
                Self::Playing {
                    song_id,
                    title,
                    artists,
                    album,
                    cover_url,
                    source_url,
                    duration_ms,
                    start_epoch_ms,
                },
                false,
            ) => {
                let desired_cover = initial_cover_url(&desired.song);
                *song_id == desired.song.id
                    && *title == desired.song.title
                    && *artists == desired.song.artists
                    && *album == desired.song.album
                    && *cover_url == desired_cover
                    && *source_url == desired.song.source_url
                    && (*duration_ms - desired.duration_ms).abs() <= TIMESTAMP_DRIFT_TOLERANCE_MS
                    && (*start_epoch_ms - desired.start_epoch_ms).abs() <= TIMESTAMP_DRIFT_TOLERANCE_MS
            }
        }
    }
}

pub fn initial_cover_url(song: &Song) -> Option<String> {
    if let Some(url) = song.external_cover_url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(url.to_string());
    }
    if let Some(cover_url) = song.cover_url.as_deref().map(str::trim) {
        if cover_url.starts_with("https://") || cover_url.starts_with("http://") {
            return Some(cover_url.to_string());
        }
    }
    None
}

/// Tracks the latest desired presence state, the last state successfully pushed to Discord,
/// and enforces a cooldown/sync interval between Discord IPC writes.
pub struct PresenceWatcher {
    desired: Option<DesiredPresence>,
    last_pushed: Option<PushedPresence>,
    next_sync_at: Option<Instant>,
    sync_interval: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncAction {
    /// Desired state already matches what was pushed (or no state yet). Wait for new events.
    Idle,
    /// Out of sync, but still inside the cooldown window. Wait at most this duration.
    Wait(Duration),
    /// Out of sync and cooldown elapsed. Push `desired` now.
    SyncNow,
}

impl PresenceWatcher {
    pub fn new(sync_interval: Duration) -> Self {
        Self {
            desired: None,
            last_pushed: None,
            next_sync_at: None,
            sync_interval,
        }
    }

    pub fn set_desired(&mut self, desired: DesiredPresence) {
        self.desired = Some(desired);
    }

    pub fn desired(&self) -> Option<&DesiredPresence> {
        self.desired.as_ref()
    }

    pub fn is_dirty(&self) -> bool {
        let Some(desired) = self.desired.as_ref() else {
            return false;
        };
        match self.last_pushed.as_ref() {
            None => true,
            Some(pushed) => !pushed.matches(desired),
        }
    }

    pub fn next_action(&self, now: Instant) -> SyncAction {
        if !self.is_dirty() {
            return SyncAction::Idle;
        }
        match self.next_sync_at {
            Some(next) if now < next => SyncAction::Wait(next.saturating_duration_since(now)),
            _ => SyncAction::SyncNow,
        }
    }

    pub fn record_sync_result(
        &mut self,
        now: Instant,
        synced: Option<PushedPresence>,
    ) {
        if let Some(pushed) = synced {
            self.last_pushed = Some(pushed);
        }
        self.next_sync_at = Some(now + self.sync_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Platform, QualityTier};

    fn make_song(id: Option<i64>, title: &str) -> Song {
        Song {
            id,
            path: String::new(),
            title: title.into(),
            artists: vec!["Artist".into()],
            album_artist: "Artist".into(),
            album: "Album".into(),
            duration_ms: 180_000,
            track_number: Some(1),
            disc_number: None,
            genre: None,
            release_year: None,
            cover_url: Some("https://example.com/cover.jpg".into()),
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source: Platform::Youtube,
            source_url: Some("https://music.youtube.com/watch?v=abc".into()),
            source_item_id: None,
            canonical_track_slug: String::new(),
            canonical_album_slug: String::new(),
            quality_tier: QualityTier::High,
        }
    }

    #[test]
    fn first_update_syncs_immediately_then_becomes_idle() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();
        assert_eq!(watcher.next_action(t0), SyncAction::Idle);

        let desired = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 0, false, 1_000_000);
        watcher.set_desired(desired.clone());
        assert_eq!(watcher.next_action(t0), SyncAction::SyncNow);

        let pushed = PushedPresence::from_desired(&desired, None);
        watcher.record_sync_result(t0, Some(pushed));
        assert_eq!(watcher.next_action(t0), SyncAction::Idle);
    }

    #[test]
    fn rapid_changes_wait_for_cooldown_then_sync_latest() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();

        let d1 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 0, false, 1_000_000);
        watcher.set_desired(d1.clone());
        assert_eq!(watcher.next_action(t0), SyncAction::SyncNow);
        watcher.record_sync_result(t0, Some(PushedPresence::from_desired(&d1, None)));

        // Seek 1 second later: still inside 5s cooldown
        let t1 = t0 + Duration::from_secs(1);
        let d2 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 60_000, false, 1_001_000);
        watcher.set_desired(d2);
        assert_eq!(watcher.next_action(t1), SyncAction::Wait(Duration::from_secs(4)));

        // After cooldown elapses, it syncs immediately
        let t5 = t0 + Duration::from_secs(5);
        assert_eq!(watcher.next_action(t5), SyncAction::SyncNow);
    }

    #[test]
    fn failed_sync_stays_dirty_and_retries_after_interval() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();

        let d1 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 0, false, 1_000_000);
        watcher.set_desired(d1);
        assert_eq!(watcher.next_action(t0), SyncAction::SyncNow);

        // Sync failed (None pushed)
        watcher.record_sync_result(t0, None);
        assert!(watcher.is_dirty());
        assert_eq!(watcher.next_action(t0 + Duration::from_secs(2)), SyncAction::Wait(Duration::from_secs(3)));
        assert_eq!(watcher.next_action(t0 + Duration::from_secs(5)), SyncAction::SyncNow);
    }

    #[test]
    fn normal_playback_drift_within_tolerance_is_not_dirty() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();

        let d1 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 10_000, false, 1_000_000);
        watcher.set_desired(d1.clone());
        watcher.record_sync_result(t0, Some(PushedPresence::from_desired(&d1, None)));

        // 5 seconds later, position is 15_000ms -> start_epoch_ms is still ~990_000ms
        let d2 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 15_200, false, 1_005_000);
        watcher.set_desired(d2);
        assert!(!watcher.is_dirty());
        assert_eq!(watcher.next_action(t0 + Duration::from_secs(5)), SyncAction::Idle);
    }

    #[test]
    fn paused_state_syncs_cleared_once_and_does_not_repeat() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();

        let playing = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 10_000, false, 1_000_000);
        watcher.set_desired(playing.clone());
        watcher.record_sync_result(t0, Some(PushedPresence::from_desired(&playing, None)));

        let paused1 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 10_000, true, 1_006_000);
        watcher.set_desired(paused1.clone());
        assert_eq!(watcher.next_action(t0 + Duration::from_secs(6)), SyncAction::SyncNow);
        watcher.record_sync_result(t0 + Duration::from_secs(6), Some(PushedPresence::from_desired(&paused1, None)));

        // Another paused event does not re-dirty
        let paused2 = DesiredPresence::from_playback(make_song(Some(1), "Track A"), 180_000, 10_000, true, 1_012_000);
        watcher.set_desired(paused2);
        assert!(!watcher.is_dirty());
        assert_eq!(watcher.next_action(t0 + Duration::from_secs(12)), SyncAction::Idle);
    }

    #[test]
    fn resolved_external_cover_matches_subsequent_player_echo() {
        let mut watcher = PresenceWatcher::new(Duration::from_secs(5));
        let t0 = Instant::now();

        let mut song = make_song(Some(1), "Track A");
        song.cover_url = None;
        let d1 = DesiredPresence::from_playback(song.clone(), 180_000, 0, false, 1_000_000);
        watcher.set_desired(d1.clone());
        watcher.record_sync_result(
            t0,
            Some(PushedPresence::from_desired(&d1, Some("https://cdn.example.com/found.jpg".into()))),
        );

        // Player receives apply_external_cover and echoes update back
        song.external_cover_url = Some("https://cdn.example.com/found.jpg".into());
        let d2 = DesiredPresence::from_playback(song, 180_000, 200, false, 1_000_200);
        watcher.set_desired(d2);
        assert!(!watcher.is_dirty());
    }
}
