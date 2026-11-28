use super::request_bytes;
use std::time::Duration;
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
async fn request_bytes_retries_503_and_returns_success_body() {
    let (url, server) = response_server(vec![
        b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    ])
    .await;
    let client = loopback_client();
    let body = request_bytes(
        || client.get(&url),
        Duration::from_secs(3),
        64,
    )
    .await
    .expect("second successful response should be returned");

    timeout(Duration::from_secs(2), server)
        .await
        .expect("server should handle both requests")
        .expect("server should not panic");
    assert_eq!(body, b"ok");
}

#[tokio::test]
async fn request_bytes_returns_404_without_retrying() {
    let (url, server) = response_server(vec![
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ])
    .await;
    let client = loopback_client();
    let error = request_bytes(
        || client.get(&url),
        Duration::from_secs(3),
        64,
    )
    .await
    .expect_err("404 should fail");

    timeout(Duration::from_secs(2), server)
        .await
        .expect("server should handle one request")
        .expect("server should not panic");
    assert!(error.contains("404"), "unexpected error: {error}");
}
