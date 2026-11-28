use super::read_text_response;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

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
    (format!("http://{address}/soundcloud?client_id=SC_SECRET_FIXTURE"), server)
}

async fn get_response(url: &str) -> reqwest::Response {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client should build")
        .get(url)
        .send()
        .await
        .expect("loopback response should arrive")
}

async fn join_server(server: JoinHandle<()>) {
    timeout(Duration::from_secs(4), server)
        .await
        .expect("fixture server should finish")
        .expect("fixture server should not panic");
}

#[tokio::test]
async fn text_response_rejects_declared_oversize_without_leaking_url_or_secret() {
    let (url, server) = response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n".to_vec(),
    )
    .await;
    let error = read_text_response(get_response(&url).await)
        .await
        .expect_err("declared oversized response should be rejected");
    join_server(server).await;

    assert!(error.message.contains("response too large"), "unexpected error: {}", error.message);
    assert!(!error.message.contains(&url));
    assert!(!error.message.contains("SC_SECRET_FIXTURE"));
}

#[tokio::test]
async fn text_response_rejects_chunked_oversize_body() {
    let mut response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n800001\r\n".to_vec();
    response.extend(vec![b'x'; 8 * 1024 * 1024 + 1]);
    response.extend_from_slice(b"\r\n0\r\n\r\n");
    let (url, server) = response_server(response).await;

    let error = read_text_response(get_response(&url).await)
        .await
        .expect_err("streamed oversized response should be rejected");
    join_server(server).await;

    assert!(error.message.contains("response too large"), "unexpected error: {}", error.message);
    assert!(!error.message.contains("SC_SECRET_FIXTURE"));
}

#[tokio::test]
async fn text_response_accepts_a_small_body() {
    let (url, server) = response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
    )
    .await;
    let body = read_text_response(get_response(&url).await)
        .await
        .expect("small response should be accepted");
    join_server(server).await;
    assert_eq!(body, "ok");
}
