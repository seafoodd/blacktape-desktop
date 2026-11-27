use crate::audio::player::{lock_player, AudioPlayer};
use crate::db::db::Database;
use crate::{
    discord_presence::{cover_fetcher::CoverFetcher, watcher::initial_cover_url},
    types::Song,
};
use discord_rich_presence::{
    activity::{self, Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
#[cfg(test)]
use std::{thread, time::Duration};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const CLIENT_ID: &str = "1490382526169219342";
const SOURCE_CODE_URL: &str = "https://github.com/seafoodd/blacktape-desktop";
const APP_NAME: &str = "Blacktape Music Player";

#[derive(Debug)]
pub enum RpcError {
    ConnectError(String),
    ActivityError(String),
    TimeError,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::ConnectError(e) => write!(f, "Connection failed: {e}"),
            RpcError::ActivityError(e) => write!(f, "Activity update failed: {e}"),
            RpcError::TimeError => write!(f, "Failed to get system time"),
        }
    }
}

impl std::error::Error for RpcError {}

type Result<T> = std::result::Result<T, RpcError>;

#[cfg(test)]
fn retry<T, E, F>(mut f: F, max_attempts: u32, delay: Duration) -> Result<T>
where
    F: FnMut() -> std::result::Result<T, E>,
    E: std::fmt::Debug,
{
    let mut attempt = 1;

    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(err) => {
                // eprintln!("Attempt {}/{} failed: {:?}", attempt, max_attempts, err);
                if attempt >= max_attempts {
                    return Err(RpcError::ConnectError(format!("{err:?}")));
                }
                thread::sleep(delay);
                attempt += 1;
            }
        }
    }
}
pub struct DiscordRpcClient {
    client: DiscordIpcClient,
    activity: Activity<'static>,
    pub is_connected: bool,
    cover_fetcher: CoverFetcher,
    handle: AppHandle,
}

impl DiscordRpcClient {
    pub fn new(handle: AppHandle) -> Result<Self> {
        let client = DiscordIpcClient::new(CLIENT_ID);
        let activity = Self::build_initial_activity()?;

        Ok(Self {
            client,
            activity,
            is_connected: false,
            cover_fetcher: CoverFetcher::new(),
            handle,
        })
    }

    fn build_initial_activity() -> Result<Activity<'static>> {
        Ok(Activity::new()
            .activity_type(activity::ActivityType::Listening)
            .name(APP_NAME))
    }

    pub fn update_song(
        &mut self,
        song: &Song,
        start_epoch_ms: i64,
        end_epoch_ms: i64,
        is_paused: bool,
    ) -> Result<Option<String>> {
        if is_paused {
            self.client
                .clear_activity()
                .map_err(|e| RpcError::ActivityError(format!("{e:?}")))?;

            return Ok(None);
        }

        let timestamps = Timestamps::new()
            .start(start_epoch_ms)
            .end(end_epoch_ms);

        let mut cover_url = initial_cover_url(song);

        if cover_url.is_none() {
            if let Some(fetched_url) = self.cover_fetcher.fetch_cover_url(song) {
                cover_url = Some(fetched_url.clone());
                if let Some(song_id) = song.id {
                    let handle = self.handle.clone();

                    tauri::async_runtime::spawn(async move {
                        let db_state = handle.state::<tokio::sync::Mutex<Database>>();
                        let db = db_state.lock().await;

                        if db
                            .update_external_cover(song_id, &fetched_url)
                            .await
                            .is_ok()
                        {
                            // straight to the player (an event could also be sent by the web view)
                            if let Some(player) = handle.try_state::<std::sync::Mutex<AudioPlayer>>() {
                                lock_player(&player).apply_external_cover(song_id, fetched_url);
                            }
                        }
                    });
                }
            }
        }

        let final_cover_url = cover_url.clone().unwrap_or_else(|| "album_generic".to_string());

        let assets = Assets::new()
            .large_image(final_cover_url)
            .large_url(SOURCE_CODE_URL)
            .large_text(song.album.to_string()); // second description line
                                                 // .small_image(APP_IMAGE_URL)
                                                 // .small_url(SOURCE_CODE_URL)
                                                 // .small_text(APP_NAME)

        let display_artists = song.artists.join(", ");

        let activity = self
            .activity
            .clone()
            .details(song.title.to_string())
            .state(display_artists) // first description line
            .assets(assets)
            .timestamps(timestamps)
            .state_url(SOURCE_CODE_URL)
            .details_url(SOURCE_CODE_URL);

        self.activity = activity.clone();

        self.client
            .set_activity(activity)
            .map_err(|e| RpcError::ActivityError(format!("{e:?}")))?;

        Ok(cover_url)
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

    pub fn reconnect(&mut self) -> Result<()> {
        if !self.is_connected {
            self.client
                .connect()
                .map_err(|e| RpcError::ConnectError(format!("{e:?}")))?;
            self.is_connected = true;
        }
        Ok(())
    }
}

pub fn current_timestamp_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|_| RpcError::TimeError)
}

impl Drop for DiscordRpcClient {
    fn drop(&mut self) {
        if self.is_connected {
            let _ = self.client.close();
            // println!("Discord RPC connection closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_mechanism_success_eventually() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        // Fail twice, succeed on the 3rd attempt
        let result = retry(
            move || {
                let current = counter_clone.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    Err("temporary failure")
                } else {
                    Ok("success!")
                }
            },
            5,
            Duration::from_millis(1),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success!");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_retry_mechanism_exhausted_fallback() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let result = retry(
            move || {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                let err: std::result::Result<(), &str> = Err("permanent blowup");
                err
            },
            3,
            Duration::from_millis(1),
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            RpcError::ConnectError(msg) => assert!(msg.contains("permanent blowup")),
            _ => panic!("Expected a ConnectError variant"),
        }
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn initial_cover_url_uses_web_cover_url_for_streamed_songs_and_skips_local_paths() {
        let mut song = Song {
            id: None,
            path: String::new(),
            title: "Streamed".into(),
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
            source: crate::types::Platform::Youtube,
            source_url: Some("https://music.youtube.com/watch?v=abc".into()),
            source_item_id: None,
            canonical_track_slug: String::new(),
            canonical_album_slug: String::new(),
            quality_tier: crate::types::QualityTier::High,
        };
        assert_eq!(
            initial_cover_url(&song).as_deref(),
            Some("https://example.com/cover.jpg")
        );

        song.external_cover_url = Some("https://cdn.example.com/ext.jpg".into());
        assert_eq!(
            initial_cover_url(&song).as_deref(),
            Some("https://cdn.example.com/ext.jpg")
        );

        song.external_cover_url = None;
        song.cover_url = Some("C:\\Music\\cover.jpg".into());
        assert_eq!(initial_cover_url(&song), None);
    }
}
