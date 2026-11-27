use super::shared_client_id;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use tokio::{sync::Mutex, time::{sleep, Duration}};

#[tokio::test]
async fn concurrent_requests_share_one_fetch() {
    let cache = Arc::new(Mutex::new(None));
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();

    for _ in 0..8 {
        let cache = Arc::clone(&cache);
        let fetch_count = Arc::clone(&fetch_count);
        tasks.push(tokio::spawn(async move {
            shared_client_id(&cache, None, move || async move {
                fetch_count.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
                Ok("shared-client-id".to_string())
            })
            .await
        }));
    }

    for task in tasks {
        assert_eq!(task.await.expect("request task should complete").unwrap(), "shared-client-id");
    }
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
    assert_eq!(*cache.lock().await, Some("shared-client-id".to_string()));
}

#[tokio::test]
async fn rejected_cached_id_is_replaced_once_and_then_reused() {
    let cache = Mutex::new(Some("old-id".to_string()));
    let fetch_count = AtomicUsize::new(0);

    let first = shared_client_id(&cache, Some("old-id"), || async {
        fetch_count.fetch_add(1, Ordering::SeqCst);
        Ok("new-id".to_string())
    })
    .await
    .unwrap();
    let second = shared_client_id(&cache, Some("old-id"), || async {
        fetch_count.fetch_add(1, Ordering::SeqCst);
        Ok("unexpected-id".to_string())
    })
    .await
    .unwrap();

    assert_eq!(first, "new-id");
    assert_eq!(second, "new-id");
    assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_fetch_does_not_poison_cache_and_can_be_retried() {
    let cache = Mutex::new(None);

    let error = shared_client_id(&cache, None, || async {
        Err::<String, _>("temporary fetch failure".to_string())
    })
    .await
    .unwrap_err();
    assert_eq!(error, "temporary fetch failure");
    assert_eq!(*cache.lock().await, None);

    let recovered = shared_client_id(&cache, None, || async { Ok("recovered-id".to_string()) })
        .await
        .unwrap();
    assert_eq!(recovered, "recovered-id");
    assert_eq!(*cache.lock().await, Some("recovered-id".to_string()));
}
