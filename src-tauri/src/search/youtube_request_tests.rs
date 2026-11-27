use super::request_json;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

async fn response_server(responses: Vec<&'static [u8]>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = timeout(Duration::from_secs(2), listener.accept())
                .await
                .expect("client should connect")
                .expect("accept should succeed");
            let mut request = [0_u8; 2048];
            timeout(Duration::from_secs(2), socket.read(&mut request))
                .await
                .expect("request read should finish")
                .expect("request read should succeed");
            timeout(Duration::from_secs(2), socket.write_all(response))
                .await
                .expect("response write should finish")
                .expect("response write should succeed");
            timeout(Duration::from_secs(2), socket.shutdown())
                .await
                .expect("socket shutdown should finish")
                .expect("socket shutdown should succeed");
        }
    });
    (format!("http://{address}/fixture"), server)
}

fn loopback_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().expect("client should build")
}

#[tokio::test]
async fn request_json_retries_transient_503_and_parses_success() {
    let (url, server) = response_server(vec![
        b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
    ])
    .await;
    let client = loopback_client();
    let value = request_json(client.get(&url), Duration::from_secs(3))
        .await
        .expect("retry should return successful JSON");

    timeout(Duration::from_secs(2), server)
        .await
        .expect("server should handle both attempts")
        .expect("server should not panic");
    assert_eq!(value, serde_json::json!({"ok": true}));
}

#[tokio::test]
async fn request_json_does_not_retry_when_retry_after_exceeds_budget() {
    let (url, server) = response_server(vec![
        b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 10\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ])
    .await;
    let client = loopback_client();
    let started = Instant::now();
    let error = request_json(client.get(&url), Duration::from_millis(40))
        .await
        .expect_err("long retry delay should exceed budget");

    timeout(Duration::from_secs(2), server)
        .await
        .expect("server should handle the first attempt only")
        .expect("server should not panic");
    assert!(error.contains("503"), "unexpected error: {error}");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn request_json_deadline_bounds_server_that_never_sends_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("client should connect");
        let mut request = [0_u8; 2048];
        let _ = socket.read(&mut request).await;
        let _socket = socket;
        std::future::pending::<()>().await;
    });
    let client = loopback_client();
    let started = Instant::now();
    let result = request_json(
        client.get(format!("http://{address}/hang")),
        Duration::from_millis(40),
    )
    .await;
    server.abort();

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}
