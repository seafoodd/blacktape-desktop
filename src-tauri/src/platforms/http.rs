use reqwest::{Client, Response};
use std::sync::OnceLock;
use std::time::Duration;

const PAGE_LIMIT: usize = 8 * 1024 * 1024;
const COVER_LIMIT: usize = 12 * 1024 * 1024;

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
pub async fn read_bytes_limited(mut response: Response, max: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    if response.content_length().is_some_and(|length| length > max as u64) {
        return Err("response too large".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.without_url().to_string())? {
        if body.len().saturating_add(chunk.len()) > max {
            return Err("response too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn get_page(raw: &str) -> Result<String, String> {
    let url = checked_public_url(raw)?;
    let response = client()?.get(url).send().await.map_err(|error| error.without_url().to_string())?;
    let bytes = read_bytes_limited(response, PAGE_LIMIT).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn get_cover(raw: &str) -> Result<Vec<u8>, String> {
    let url = checked_public_url(raw)?;
    let response = client()?.get(url).send().await.map_err(|error| error.without_url().to_string())?;
    read_bytes_limited(response, COVER_LIMIT).await
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
