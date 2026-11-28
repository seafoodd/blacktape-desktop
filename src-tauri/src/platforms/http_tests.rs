use super::{checked_public_url, read_bytes_limited};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener, time::{timeout, Duration}};

#[test]
fn checked_public_url_accepts_https_public_domains() {
    for raw in [
        "https://example.com",
        "https://music.artist.com/path?q=1",
        "https://example.com:443/path",
    ] {
        assert!(checked_public_url(raw).is_ok(), "URL: {raw}");
    }
}

#[test]
fn checked_public_url_rejects_non_public_or_unsupported_urls() {
    for raw in [
        "http://example.com",
        "ftp://example.com/file",
        "https://user@example.com",
        "https://user:pass@example.com",
        "https://example.com:444",
        "https://127.0.0.1/",
        "https://[::1]/",
        "https://localhost/",
        "https://api.localhost/",
        "https://printer.local/",
        "https://service.internal/",
        "https://example.test/",
        "https://example.invalid/",
        "https://intranet/",
        "not a URL",
    ] {
        assert!(checked_public_url(raw).is_err(), "URL should be rejected: {raw}");
    }
}

async fn one_response_server(response: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = timeout(Duration::from_secs(3), listener.accept())
            .await
            .expect("client did not connect")
            .expect("accept failed");
        let mut request = [0_u8; 2048];
        let _ = timeout(Duration::from_secs(3), socket.read(&mut request)).await;
        let _ = timeout(Duration::from_secs(3), socket.write_all(response)).await;
        let _ = timeout(Duration::from_secs(3), socket.shutdown()).await;
    });
    format!("http://{address}/test")
}

#[tokio::test]
async fn read_bytes_limited_rejects_non_success_status_before_body() {
    let endpoint = one_response_server(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
    )
    .await;
    let response = reqwest::Client::new().get(endpoint).send().await.unwrap();

    let error = read_bytes_limited(response, 32).await.unwrap_err();
    assert!(error.contains("404"), "unexpected error: {error}");
}

#[tokio::test]
async fn read_bytes_limited_rejects_declared_oversized_body() {
    let endpoint = one_response_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx",
    )
    .await;
    let response = reqwest::Client::new().get(endpoint).send().await.unwrap();

    assert!(read_bytes_limited(response, 8).await.is_err());
}

#[tokio::test]
async fn read_bytes_limited_caps_streamed_body_without_content_length() {
    let endpoint = one_response_server(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
    )
    .await;
    let response = reqwest::Client::new().get(endpoint).send().await.unwrap();

    assert!(read_bytes_limited(response, 4).await.is_err());
}
