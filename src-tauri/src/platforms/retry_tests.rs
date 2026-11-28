use super::{retrying_for, Failure};
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, SystemTime}};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener, time::{timeout, Instant}};

#[tokio::test]
async fn retry_after_delay_overrides_backoff() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_by_attempt = Arc::clone(&calls);
    let started = Instant::now();
    let result = retrying_for(Duration::from_secs(1), 2, |_| Duration::ZERO, move || {
        let call = calls_by_attempt.fetch_add(1, Ordering::SeqCst);
        async move {
            if call == 0 {
                Err(Failure { message: "busy".into(), retry: true, retry_after: Some(Duration::from_millis(20)) })
            } else {
                Ok("done")
            }
        }
    })
    .await;

    assert_eq!(result.unwrap(), "done");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(started.elapsed() >= Duration::from_millis(15));
}

#[tokio::test]
async fn retry_delay_beyond_budget_does_not_start_another_attempt() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_by_attempt = Arc::clone(&calls);
    let started = Instant::now();
    let result: Result<(), String> = retrying_for(
        Duration::from_millis(10),
        3,
        |_| Duration::ZERO,
        move || {
            calls_by_attempt.fetch_add(1, Ordering::SeqCst);
            async {
                Err(Failure {
                    message: "busy".into(),
                    retry: true,
                    retry_after: Some(Duration::from_secs(1)),
                })
            }
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[tokio::test]
async fn retrying_for_bounds_a_hanging_attempt_by_the_budget() {
    let started = Instant::now();
    let result: Result<(), String> = retrying_for(
        Duration::from_millis(20),
        3,
        |_| Duration::ZERO,
        || std::future::pending(),
    )
    .await;

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[tokio::test]
async fn fatal_failure_is_not_retried() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_by_attempt = Arc::clone(&calls);
    let result: Result<(), String> = retrying_for(Duration::from_secs(1), 3, |_| Duration::ZERO, move || {
        calls_by_attempt.fetch_add(1, Ordering::SeqCst);
        async { Err(Failure { message: "HTTP 404".into(), retry: false, retry_after: None }) }
    })
    .await;

    assert_eq!(result.unwrap_err(), "HTTP 404");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

async fn response_with_header(header: (&str, String)) -> reqwest::Response {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let response = format!(
        "HTTP/1.1 429 Too Many Requests\r\n{}: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        header.0, header.1
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("test server should receive request")
            .expect("accept should succeed");
        let mut request = [0_u8; 2048];
        let _ = timeout(Duration::from_secs(2), socket.read(&mut request)).await;
        timeout(Duration::from_secs(2), socket.write_all(response.as_bytes()))
            .await
            .expect("response write should finish")
            .expect("response write should succeed");
        timeout(Duration::from_secs(2), socket.shutdown())
            .await
            .expect("socket shutdown should finish")
            .expect("socket shutdown should succeed");
    });
    let response = reqwest::Client::new()
        .get(format!("http://{address}/retry?client_id=CLIENT_ID_SECRET_VALUE"))
        .send()
        .await
        .expect("loopback fixture response should be received");
    timeout(Duration::from_secs(2), server)
        .await
        .expect("test server should finish")
        .expect("test server should not panic");
    response
}

#[tokio::test]
async fn from_response_parses_numeric_retry_after() {
    let response = response_with_header(("Retry-After", "2".into())).await;
    let failure = Failure::from_response(&response);
    assert!(failure.retry);
    assert_eq!(failure.retry_after, Some(Duration::from_secs(2)));
}

#[tokio::test]
async fn from_response_parses_http_date_retry_after() {
    let date = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
    let response = response_with_header(("Retry-After", date)).await;
    let failure = Failure::from_response(&response);
    let delay = failure.retry_after.expect("HTTP-date should produce a delay");
    assert!((58..=61).contains(&delay.as_secs()), "unexpected delay: {delay:?}");
}

#[tokio::test]
async fn response_failure_message_does_not_include_request_url_or_secrets() {
    let response = response_with_header(("Retry-After", "0".into())).await;
    let url = response.url().to_string();
    let failure = Failure::from_response(&response);
    assert!(url.contains("CLIENT_ID_SECRET_VALUE"));
    assert!(!failure.message.contains(&url));
    assert!(!failure.message.contains("CLIENT_ID_SECRET_VALUE"));
}
