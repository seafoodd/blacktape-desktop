use super::{
    album_metadata, album_tracks, artist_profile, search_dropdown, search_page, track_candidates,
};
use crate::{search::full::SearchKind, types::Platform};

#[tokio::test]
async fn every_platform_adapter_rejects_local_without_dispatching_network_work() {
    assert!(search_dropdown(Platform::Local, "query").await.is_err());
    assert!(search_page(Platform::Local, "query", SearchKind::Track, None).await.is_err());
    assert!(artist_profile(Platform::Local, "artist").await.is_err());
    assert!(album_metadata(Platform::Local, "https://example.test/album").await.is_err());
    assert!(track_candidates(Platform::Local, "artist", "track", 180_000).await.is_err());
    assert!(album_tracks(Platform::Local, "artist", "album").await.is_err());
}
