use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Debug, Deserialize)]
pub struct Song {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Duration,
    pub cover: Option<Vec<u8>>,
}
