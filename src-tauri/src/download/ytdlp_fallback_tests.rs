use super::download_with_fallbacks;
use crate::download::types::TrackDownload;
use std::{path::PathBuf, sync::{Arc, Mutex}};

fn sample_track() -> TrackDownload {
    TrackDownload {
        url: "https://youtube.com/watch?v=primary".into(),
        source_item_id: Some("source-17".into()),
        source_track_index: Some(4),
        title: "A song".into(),
        fallback_urls: vec![
            "not a URL".into(),
            "file:///tmp/invalid.mp3".into(),
            "https://soundcloud.com/artist/song".into(),
        ],
        ..Default::default()
    }
}

#[tokio::test]
async fn fallback_download_retries_valid_urls_and_returns_selected_source_metadata() {
    let track = sample_track();
    let attempted = Arc::new(Mutex::new(Vec::new()));
    let attempted_by_closure = Arc::clone(&attempted);
    let result = download_with_fallbacks(&track, move |candidate| {
        attempted_by_closure.lock().unwrap().push(candidate.url.clone());
        async move {
            if candidate.url == "https://soundcloud.com/artist/song" {
                Some(PathBuf::from("saved.mp3"))
            } else {
                None
            }
        }
    })
    .await
    .expect("fallback attempt should succeed");

    assert_eq!(
        *attempted.lock().unwrap(),
        vec![
            "https://youtube.com/watch?v=primary",
            "https://soundcloud.com/artist/song"
        ]
    );
    assert_eq!(result.0, PathBuf::from("saved.mp3"));
    assert_eq!(result.1.url, "https://soundcloud.com/artist/song");
    assert_eq!(result.1.source_item_id, None);
    assert_eq!(result.1.source_track_index, Some(4));
    assert_eq!(result.1.title, "A song");
}

#[tokio::test]
async fn primary_success_preserves_original_source_metadata() {
    let track = sample_track();
    let result = download_with_fallbacks(&track, |_candidate| async {
        Some(PathBuf::from("primary.mp3"))
    })
    .await
    .expect("primary attempt should succeed");

    assert_eq!(result.1.url, track.url);
    assert_eq!(result.1.source_item_id.as_deref(), Some("source-17"));
    assert_eq!(result.1.source_track_index, Some(4));
}
