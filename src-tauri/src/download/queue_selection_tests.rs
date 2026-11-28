use super::*;
use std::collections::HashMap;
use std::sync::Arc;

fn state(id: &str) -> ActiveDownloadState {
    ActiveDownloadState {
        id: id.to_string(),
        platform: "test".into(),
        download_type: "album".into(),
        url: String::new(),
        artist_name: String::new(),
        album_title: String::new(),
        cover_url: None,
        release_year: None,
        status: "queued".into(),
        current: 0,
        total: 3,
        overall_percent: 0,
        message: String::new(),
        tracks: Vec::new(),
    }
}

async fn torrent_fixture() -> (tempfile::TempDir, Arc<librqbit::Session>, Arc<librqbit::ManagedTorrent>) {
    use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions};

    let directory = tempfile::tempdir().unwrap();
    let session = Session::new_with_opts(
        directory.path().to_owned(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            disable_upload: true,
            persistence: None,
            listen_port_range: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let mut metadata = b"d4:infod5:filesl".to_vec();
    for name in ["a.flac", "b.flac", "c.flac"] {
        metadata.extend_from_slice(format!("d6:lengthi3e4:pathl{}:{}ee", name.len(), name).as_bytes());
    }
    metadata.extend_from_slice(b"e4:name4:album12:piece lengthi16384e6:pieces20:");
    metadata.extend_from_slice(&[0; 20]);
    metadata.extend_from_slice(b"ee");

    let handle = session
        .add_torrent(AddTorrent::from_bytes(metadata), Some(AddTorrentOptions::default()))
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    handle.wait_until_initialized().await.unwrap();
    (directory, session, handle)
}

fn track_to_file() -> HashMap<usize, usize> {
    HashMap::from([(0, 0), (1, 1), (2, 2)])
}

#[tokio::test]
async fn cancellation_before_torrent_registration_is_applied_to_new_selection() {
    let (_directory, session, handle) = torrent_fixture().await;
    let registry = TaskRegistry::default();
    registry.register_task(state("cancel-before-register")).await;
    assert_eq!(registry.cancel_track("cancel-before-register", 0).await, CancelTrackOutcome::Cancelled);

    registry
        .register_torrent_selection("cancel-before-register", session.clone(), handle.clone(), track_to_file())
        .await;

    let mut selected = handle.only_files().unwrap();
    selected.sort_unstable();
    assert_eq!(selected.into_iter().collect::<std::collections::HashSet<_>>(), [1, 2].into());
    session.stop().await;
}

#[tokio::test]
async fn concurrent_cancellations_cannot_be_overwritten_by_stale_file_selection() {
    let (_directory, session, handle) = torrent_fixture().await;
    let registry = TaskRegistry::default();
    registry.register_task(state("concurrent-cancel")).await;
    registry
        .register_torrent_selection("concurrent-cancel", session.clone(), handle.clone(), track_to_file())
        .await;

    let sync_lock = {
        let tasks = registry.tasks.lock().await;
        Arc::clone(&tasks.get("concurrent-cancel").unwrap().torrent_selection_sync)
    };
    let guard = sync_lock.lock().await;
    let first = tokio::spawn({
        let registry = registry.clone();
        async move { registry.cancel_track("concurrent-cancel", 0).await }
    });
    let second = tokio::spawn({
        let registry = registry.clone();
        async move { registry.cancel_track("concurrent-cancel", 1).await }
    });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !registry.is_track_cancelled("concurrent-cancel", 0).await
            || !registry.is_track_cancelled("concurrent-cancel", 1).await
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    drop(guard);
    assert_eq!(first.await.unwrap(), CancelTrackOutcome::Cancelled);
    assert_eq!(second.await.unwrap(), CancelTrackOutcome::Cancelled);
    let mut selected = handle.only_files().unwrap();
    selected.sort_unstable();
    assert_eq!(selected.into_iter().collect::<std::collections::HashSet<_>>(), [2].into());
    session.stop().await;
}
