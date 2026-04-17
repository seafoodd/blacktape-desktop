use crate::db::db::Database;
use crate::{discord_presence::cover_fetcher::CoverFetcher, types::Song};
use discord_rich_presence::{
    activity::{self, Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const CLIENT_ID: &str = "1490382526169219342";
const SOURCE_CODE_URL: &str = "https://github.com/seafoodd/blacktape-desktop";
const APP_NAME: &str = "Blacktape";
const MAX_RETRIES: u32 = 10;
const RETRY_DELAY: Duration = Duration::from_secs(1);

// Custom error type
#[derive(Debug)]
pub enum RpcError {
    ConnectError(String),
    ActivityError(String),
    TimeError,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::ConnectError(e) => write!(f, "Connection failed: {}", e),
            RpcError::ActivityError(e) => write!(f, "Activity update failed: {}", e),
            RpcError::TimeError => write!(f, "Failed to get system time"),
        }
    }
}

impl std::error::Error for RpcError {}

type Result<T> = std::result::Result<T, RpcError>;

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
                    return Err(RpcError::ConnectError(format!("{:?}", err)));
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

    pub fn update_song(&mut self, song: &Song, duration_ms: i64, position_ms: i64) -> Result<()> {
        let now = current_timestamp_ms()?;

        let timestamps = Timestamps::new()
            .start(now - position_ms)
            .end(now + duration_ms - position_ms);

        let mut cover_url = song.external_cover_url.clone();

        if cover_url.is_none() {
            if let Some(fetched_url) = self.cover_fetcher.fetch_cover_url(song) {
                cover_url = Some(fetched_url.clone());
                let Some(song_id) = song.id else { return Ok(()) };
                let handle = self.handle.clone();
                let url_to_save = fetched_url.clone();

                tauri::async_runtime::spawn(async move {
                    let db_state = handle.state::<tokio::sync::Mutex<Database>>();
                    let db = db_state.lock().await;

                    if let Err(e) = db.update_external_cover(song_id, &url_to_save).await {
                        eprintln!("Failed to save cover to DB: {}", e);
                    }
                });
            }
        }

        let final_cover_url = cover_url.unwrap_or_else(|| "album_generic".to_string());

        let assets = Assets::new()
            .large_image(final_cover_url)
            .large_url(SOURCE_CODE_URL)
            .large_text(song.album.to_string()); // second description line
                                                 // .small_image(APP_IMAGE_URL)
                                                 // .small_url(SOURCE_CODE_URL)
                                                 // .small_text(APP_NAME)

        let activity = self
            .activity
            .clone()
            .details(song.title.to_string())
            .state(song.artist.to_string()) // first description line
            .assets(assets)
            .timestamps(timestamps)
            .state_url(SOURCE_CODE_URL)
            .details_url(SOURCE_CODE_URL);

        self.activity = activity.clone();

        retry(
            || {
                self.client
                    .set_activity(activity.clone())
                    .map_err(|e| RpcError::ActivityError(format!("{:?}", e)))
            },
            MAX_RETRIES,
            RETRY_DELAY,
        )?;

        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected
    }

    pub fn reconnect(&mut self) -> Result<()> {
        if !self.is_connected {
            // println!("Reconnecting to Discord RPC...");
            retry(
                || {
                    self.client
                        .connect()
                        .map_err(|e| RpcError::ConnectError(format!("{:?}", e)))
                },
                MAX_RETRIES,
                RETRY_DELAY,
            )?;
            self.is_connected = true;
            // println!("Reconnected");
        }
        Ok(())
    }
}

impl Drop for DiscordRpcClient {
    fn drop(&mut self) {
        if self.is_connected {
            let _ = self.client.close();
            // println!("Discord RPC connection closed");
        }
    }
}

fn current_timestamp_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|_| RpcError::TimeError)
}
