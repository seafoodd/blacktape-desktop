use super::acquire_download_slot;
use std::{sync::Arc, time::Duration};
use tokio::{sync::{oneshot, Semaphore}, time::timeout};

#[tokio::test]
async fn sign_in_wait_does_not_consume_a_download_slot() {
    let semaphore = Arc::new(Semaphore::new(1));
    let (sign_in_tx, sign_in_rx) = oneshot::channel::<()>();
    let waiting_semaphore = Arc::clone(&semaphore);
    let waiting = tokio::spawn(async move {
        acquire_download_slot(async move {
            sign_in_rx.await.map_err(|error| error.to_string())
        }, waiting_semaphore)
        .await
    });

    let permit = timeout(Duration::from_secs(1), Arc::clone(&semaphore).acquire_owned())
        .await
        .expect("sign-in wait should not occupy a slot")
        .expect("semaphore should remain open");
    drop(permit);
    sign_in_tx.send(()).expect("sign-in receiver should still be waiting");
    let permit = timeout(Duration::from_secs(1), waiting)
        .await
        .expect("slot acquisition should finish")
        .expect("worker task should not panic")
        .expect("sign-in should succeed");
    drop(permit);
}

#[tokio::test]
async fn sign_in_failure_leaves_download_slot_available() {
    let semaphore = Arc::new(Semaphore::new(1));
    let error = acquire_download_slot(async { Err::<(), _>("sign in failed".to_string()) }, Arc::clone(&semaphore))
        .await
        .unwrap_err();
    assert_eq!(error, "sign in failed");

    let permit = timeout(Duration::from_secs(1), semaphore.acquire_owned())
        .await
        .expect("failed sign-in should not consume a slot")
        .expect("semaphore should remain open");
    drop(permit);
}
