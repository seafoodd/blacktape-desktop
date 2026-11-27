pub mod cover_fetcher;
pub mod discord_rpc;
pub mod watcher;
pub use discord_rpc::DiscordRpcClient;
pub use watcher::{DesiredPresence, PresenceWatcher, PushedPresence, SyncAction, DISCORD_SYNC_INTERVAL};
