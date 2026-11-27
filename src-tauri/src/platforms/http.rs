use reqwest::{Client, Response};
use std::sync::OnceLock;
use std::time::Duration;
use std::future::Future;

const PAGE_LIMIT: usize = 8 * 1024 * 1024;
const COVER_LIMIT: usize = 12 * 1024 * 1024;

pub struct Failure {
    pub message: String,
    pub retry: bool,
    pub retry_after: Option<Duration>,
}

impl Failure {
    pub fn fatal(message: impl Into<String>) -> Self {
        Self { message: message.into(), retry: false, retry_after: None }
    }

    pub fn from_response(response: &Response) -> Self {
        let status = response.status();
        let retry_after = response.headers().get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                value.trim().parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                    httpdate::parse_http_date(value).ok().map(|date| {
                        date.duration_since(std::time::SystemTime::now()).unwrap_or_default()
                    })
                })
            });
        Self { message: format!("HTTP {status}"), retry: status.is_server_error() || status.as_u16() == 429, retry_after }
    }
}

impl From<reqwest::Error> for Failure {
    fn from(error: reqwest::Error) -> Self {
        let retry = match error.status() {
            Some(status) => status.is_server_error() || status.as_u16() == 429,
            None => error.is_timeout() || error.is_connect() || error.is_body() || error.is_request() || error.is_decode(),
        };
        Self { message: error.without_url().to_string(), retry, retry_after: None }
    }
}

pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(3u64.saturating_pow(attempt.saturating_sub(1)))
}

pub async fn retrying<T, F, Fut>(attempts: u32, pause: impl Fn(u32) -> Duration, attempt: F) -> Result<T, String>
where F: FnMut() -> Fut, Fut: Future<Output = Result<T, Failure>> {
    retrying_for(Duration::from_secs(10 * 60), attempts, pause, attempt).await
}

pub async fn retrying_for<T, F, Fut>(
    budget: Duration,
    attempts: u32,
    pause: impl Fn(u32) -> Duration,
    mut attempt: F,
) -> Result<T, String>
where F: FnMut() -> Fut, Fut: Future<Output = Result<T, Failure>> {
    let deadline = tokio::time::Instant::now() + budget;
    let mut number = 1;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("Request deadline exceeded".into());
        }
        let result = tokio::time::timeout_at(deadline, attempt()).await
            .map_err(|_| "Request deadline exceeded".to_string())?;
        match result {
            Ok(value) => return Ok(value),
            Err(failure) if failure.retry && number < attempts => {
                let delay = failure.retry_after.unwrap_or_else(|| pause(number));
                if delay >= deadline.saturating_duration_since(tokio::time::Instant::now()) {
                    return Err(failure.message);
                }
                tracing::warn!(attempt = number, error = %failure.message, "Request failed, retrying");
                tokio::time::sleep(delay).await;
                number += 1;
            }
            Err(failure) => return Err(failure.message),
        }
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
#[path = "request_tests.rs"]
mod request_tests;

/// Custom artist domains are supported, but local addresses and embedded credentials are rejected.
pub fn checked_public_url(raw: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "Invalid page URL".to_string())?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("Pages require HTTPS without credentials or a custom port".into());
    }
    let Some(url::Host::Domain(host)) = url.host() else {
        return Err("Page host must be a public domain".into());
    };
    let host = host.trim_end_matches('.');
    if !host.contains('.') || ["localhost", "local", "internal", "test", "invalid"]
        .iter().any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}"))) {
        return Err("Page host must be a public domain".into());
    }
    Ok(url)
}

pub fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT.get_or_init(|| Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || checked_public_url(attempt.url().as_str()).is_err() {
                attempt.error("Page redirect is not allowed")
            } else {
                attempt.follow()
            }
        }))
        .build().map_err(|error| error.without_url().to_string()))
        .as_ref().map_err(Clone::clone)
}

/// Status and size checks are shared by page, image and lyrics readers.
pub async fn read_bytes_limited(response: Response, max: usize) -> Result<Vec<u8>, String> {
    read_bytes(response, max).await.map_err(|failure| failure.message)
}

async fn read_bytes(mut response: Response, max: usize) -> Result<Vec<u8>, Failure> {
    if !response.status().is_success() {
        return Err(Failure::from_response(&response));
    }
    if response.content_length().is_some_and(|length| length > max as u64) {
        return Err(Failure::fatal("response too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > max {
            return Err(Failure::fatal("response too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn get_page(raw: &str) -> Result<String, String> {
    let url = checked_public_url(raw)?;
    let client = client()?;
    let bytes = request_bytes(|| client.get(url.clone()), Duration::from_secs(6), PAGE_LIMIT).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn post_page_json<T: serde::Serialize>(raw: &str, body: &T) -> Result<String, String> {
    let url = checked_public_url(raw)?;
    let client = client()?;
    let bytes = request_bytes(|| client.post(url.clone()).json(body), Duration::from_secs(6), PAGE_LIMIT).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn request_bytes(
    mut request: impl FnMut() -> reqwest::RequestBuilder,
    budget: Duration,
    max: usize,
) -> Result<Vec<u8>, String> {
    retrying_for(budget, 2, backoff, || {
        let request = request();
        async move { read_bytes(request.send().await?, max).await }
    }).await
}

pub async fn get_cover(raw: &str) -> Result<Vec<u8>, String> {
    let url = checked_public_url(raw)?;
    let client = client()?;
    request_bytes(|| client.get(url.clone()), Duration::from_secs(10), COVER_LIMIT).await
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
