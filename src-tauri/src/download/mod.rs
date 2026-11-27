pub mod album;
pub mod album_preview;
pub mod cookies;
pub mod deno;
pub(crate) mod discovery;
pub mod ffmpeg;
pub mod opus_source;
pub mod orchestrator;
pub mod post_processor;
pub mod quality;
pub mod queue;
pub(crate) mod source_selection;
pub mod tools;
pub mod track;
pub mod types;
pub mod youtube;
mod youtube_sign_in;
pub mod ytdlp;

pub use queue::{
    init_queue_worker, ActiveDownloadState, DownloadQueue, TaskRegistry, TrackProgressState,
};
pub use types::*;
