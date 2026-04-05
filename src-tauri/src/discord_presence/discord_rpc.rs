use discord_rich_presence::{
    activity::{self, Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{discord_presence::cover_fetcher::CoverFetcher, types::Song};

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
                eprintln!("Attempt {}/{} failed: {:?}", attempt, max_attempts, err);
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
    is_connected: bool,
    cover_fetcher: CoverFetcher,
}

impl DiscordRpcClient {
    pub fn new() -> Result<Self> {
        println!("Connecting to Discord RPC...");

        let mut client = DiscordIpcClient::new(CLIENT_ID);
        retry(
            || {
                client
                    .connect()
                    .map_err(|e| RpcError::ConnectError(format!("{:?}", e)))
            },
            MAX_RETRIES,
            RETRY_DELAY,
        )?;
        println!("Connected to Discord IPC");

        let activity = Self::build_initial_activity()?;

        retry(
            || {
                client
                    .set_activity(activity.clone())
                    .map_err(|e| RpcError::ActivityError(format!("{:?}", e)))
            },
            MAX_RETRIES,
            RETRY_DELAY,
        )?;
        println!("Discord Rich Presence initialized");

        Ok(Self {
            client,
            activity,
            is_connected: true,
            cover_fetcher: CoverFetcher::new(),
        })
    }

    fn build_initial_activity() -> Result<Activity<'static>> {
        Ok(Activity::new()
            .activity_type(activity::ActivityType::Listening)
            .name(APP_NAME))
    }

    pub fn update_timestamps(&mut self, position_ms: i64, duration_ms: i64) -> Result<()> {
        let now = current_timestamp_ms()?;

        let timestamps = Timestamps::new()
            .start(now - position_ms)
            .end(now + duration_ms - position_ms);

        let activity = self.activity.clone().timestamps(timestamps);
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

    pub fn update_song(&mut self, song: &Song, duration_ms: i64) -> Result<()> {
        let now = current_timestamp_ms()?;

        let timestamps = Timestamps::new().start(now).end(now + duration_ms);
        let cover_url = {
            self.cover_fetcher
                .fetch_cover_url(song)
                .unwrap_or_else(|| "album_generic".to_string())
        };

        let assets = Assets::new()
            .large_image(cover_url.to_string())
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
            println!("Reconnecting to Discord RPC...");
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
            println!("Reconnected");
        }
        Ok(())
    }
}

impl Drop for DiscordRpcClient {
    fn drop(&mut self) {
        if self.is_connected {
            let _ = self.client.close();
            println!("Discord RPC connection closed");
        }
    }
}

fn current_timestamp_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .map_err(|_| RpcError::TimeError)
}
