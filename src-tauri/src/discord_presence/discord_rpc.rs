use discord_rich_presence::{
    activity::{self, Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn retry<T, E, F>(mut f: F, attempts: u32, delay: Duration) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
    E: std::fmt::Debug,
{
    for attempt in 1..=attempts {
        match f() {
            Ok(val) => return Ok(val),
            Err(err) => {
                eprintln!("Attempt {}/{} failed: {:?}", attempt, attempts, err);
                thread::sleep(delay);
            }
        }
    }
    f()
}

pub struct DiscordRpcClient<'a> {
    client: DiscordIpcClient, // keep alive
    activity: Activity<'a>,
}

impl<'a> DiscordRpcClient<'a> {
    pub fn new() -> Self {
        println!("Connecting to Discord RPC...");

        let mut client = DiscordIpcClient::new("1490382526169219342");

        retry(|| client.connect(), 10, Duration::from_secs(1))
            .expect("Failed to connect to Discord RPC");
        println!("✅ Connected to Discord IPC.");

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let pos = 5 * 60 * 1000;
        let duration = 18 * 60 * 1000 + 17 * 1000;

        let activity = activity::Activity::new()
            .state("Bull of Heaven")
            .details("111: Superstring Theory Verified")
            .activity_type(activity::ActivityType::Listening)
            .assets(
                Assets::new()
                    .large_image("https://f4.bcbits.com/img/a3445876630_2.jpg")
                    .large_url("https://github.com/seafoodd/blacktape-desktop"),
            )
            .name("Blacktape")
            .timestamps(Timestamps::new().start(now_ms - pos).end(now_ms + duration));

        retry(
            || client.set_activity(activity.clone()),
            10,
            Duration::from_secs(1),
        )
        .expect("Failed to set initial activity");

        println!("✅ Discord Rich Presence set!");

        Self { client, activity }
    }

    pub fn update_activity(&mut self, state: &str) {
        let payload = activity::Activity::new().state(state);
        if let Err(err) = self.client.set_activity(payload) {
            println!("Failed to update activity: {:?}", err);
        }
    }

    pub fn update_timestamps(&mut self, duration: i64, position: i64) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        println!("Updating timestamp to {}, {}", duration, position);
        self.activity = self.activity.clone().timestamps(
            Timestamps::new()
                .start(now_ms - position)
                .end(now_ms + duration - position),
        );
        retry(
            || self.client.set_activity(self.activity.clone()),
            10,
            Duration::from_secs(1),
        );
    }
}
