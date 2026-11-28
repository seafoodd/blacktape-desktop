use super::request_json;
use std::time::Duration;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener, task::JoinHandle, time::timeout};

async fn response_server(response: Vec<u8>) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("client should connect")
            .expect("accept should succeed");
        let mut request = [0_u8; 2048];
        let _ = timeout(Duration::from_secs(2), socket.read(&mut request)).await;
        let _ = timeout(Duration::from_secs(3), socket.write_all(&response)).await;
        let _ = timeout(Duration::from_secs(2), socket.shutdown()).await;
    });
    (format!("http://{address}/youtube?client_id=YT_SECRET_FIXTURE"), server)
}

async fn join_server(server: JoinHandle<()>) {
    timeout(Duration::from_secs(4), server)
        .await
        .expect("fixture server should finish")
        .expect("fixture server should not panic");
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().expect("client should build")
}

#[tokio::test]
async fn request_json_rejects_declared_oversize_without_leaking_url_or_secret() {
    let (url, server) = response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_vec(),
    )
    .await;
    let error = request_json(client().get(&url), Duration::from_secs(2))
        .await
        .expect_err("declared oversized response should be rejected");
    join_server(server).await;

    assert!(error.contains("response too large"), "unexpected error: {error}");
    assert!(!error.contains(&url));
    assert!(!error.contains("YT_SECRET_FIXTURE"));
}

#[tokio::test]
async fn request_json_accepts_a_small_json_body() {
    let body = br#"{"ok":true}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    let (url, server) = response_server(response.into_bytes()).await;

    let value = request_json(client().get(&url), Duration::from_secs(2))
        .await
        .expect("small JSON response should parse");
    join_server(server).await;
    assert_eq!(value, serde_json::json!({"ok": true}));
}
