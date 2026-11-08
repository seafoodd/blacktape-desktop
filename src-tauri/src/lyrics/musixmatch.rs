//! Musixmatch provider — best coverage, supports synced (LRC) lyrics.

use super::common::{
    build_url, get_json, is_acceptable, is_trusted_https_url, LyricsSource, BROWSER_USER_AGENT,
};
use serde_json::Value;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://apic.musixmatch.com/ws/1.1";
const APP_ID: &str = "android-player-v1.0";
const SOURCE: &str = "https://www.musixmatch.com";
const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

static TOKEN_CACHE: LazyLock<Mutex<Option<(String, Instant)>>> = LazyLock::new(|| Mutex::new(None));

fn cached_token() -> Option<String> {
    let guard = TOKEN_CACHE.lock().ok()?;
    guard
        .as_ref()
        .filter(|(_, at)| at.elapsed() < TOKEN_TTL)
        .map(|(t, _)| t.clone())
}

fn store_token(token: &str) {
    if let Ok(mut g) = TOKEN_CACHE.lock() {
        *g = Some((token.to_string(), Instant::now()));
    }
}

fn invalidate_token() {
    if let Ok(mut g) = TOKEN_CACHE.lock() {
        *g = None;
    }
}

async fn get_token(client: &reqwest::Client) -> Result<String, String> {
    if let Some(t) = cached_token() {
        return Ok(t);
    }
    let url = build_url(&format!("{API}/token.get"), &[("app_id", APP_ID)]);
    let val: Value = get_json(client, &url, BROWSER_USER_AGENT).await?;
    let token = val
        .pointer("/message/body/user_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Failed to parse Musixmatch user token".to_string())?;
    store_token(token);
    Ok(token.to_string())
}

/// Removes the "NOT for Commercial use" footer Musixmatch appends to plain lyrics.
pub fn clean_musixmatch_lyrics(text: &str) -> String {
    let cut = text
        .find("******* This Lyrics is NOT for Commercial use")
        .unwrap_or(text.len());
    text[..cut].trim().to_string()
}

async fn call(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    extra: &[(&str, &str)],
) -> Option<Value> {
    let mut params: Vec<(&str, &str)> = vec![("app_id", APP_ID), ("usertoken", token)];
    params.extend_from_slice(extra);
    let url = build_url(&format!("{API}/{endpoint}"), &params);
    get_json(client, &url, BROWSER_USER_AGENT).await.ok()
}

fn body_text<'a>(val: &'a Value, path: &str) -> Option<&'a str> {
    val.pointer(&format!("/message/body/{path}"))?.as_str()
}

fn accept(raw: Option<&str>, source: &str) -> Option<LyricsSource> {
    let cleaned = clean_musixmatch_lyrics(raw?);
    is_acceptable(&cleaned).then(|| LyricsSource {
        lyrics: cleaned,
        source: source.to_string(),
    })
}

pub async fn fetch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<LyricsSource, String> {
    let token = get_token(client).await?;
    let q = [("q_artist", artist), ("q_track", title)];

    // 1. matcher.subtitle.get — timed lyrics (LRC)
    let mut p = q.to_vec();
    p.push(("subtitle_format", "lrc"));
    if let Some(v) = call(client, "matcher.subtitle.get", &token, &p).await {
        if let Some(l) = accept(body_text(&v, "subtitle/subtitle_body"), SOURCE) {
            return Ok(l);
        }
    }

    // 2. track.search -> track.subtitle.get (still prefer timed lyrics)
    let mut track_id: Option<String> = None;
    let mut share_url = SOURCE.to_string();
    let mut sp = q.to_vec();
    sp.extend_from_slice(&[("f_has_lyrics", "1"), ("page_size", "1")]);
    if let Some(v) = call(client, "track.search", &token, &sp).await {
        if let Some(tr) = v.pointer("/message/body/track_list/0/track") {
            track_id = tr.get("track_id").and_then(|i| i.as_i64()).map(|i| i.to_string());
            if let Some(u) = tr.get("track_share_url").and_then(|u| u.as_str()) {
                // Shown/opened in the UI: only accept real musixmatch https links.
                if is_trusted_https_url(u, &["musixmatch.com"]) {
                    share_url = u.to_string();
                }
            }
        }
    }
    if let Some(tid) = track_id.as_deref() {
        let p = [("track_id", tid), ("subtitle_format", "lrc")];
        if let Some(v) = call(client, "track.subtitle.get", &token, &p).await {
            if let Some(l) = accept(body_text(&v, "subtitle/subtitle_body"), &share_url) {
                return Ok(l);
            }
        }
    }

    // 3. Plain lyrics fallbacks
    if let Some(v) = call(client, "matcher.lyrics.get", &token, &q).await {
        if let Some(l) = accept(body_text(&v, "lyrics/lyrics_body"), SOURCE) {
            return Ok(l);
        }
    }
    if let Some(tid) = track_id.as_deref() {
        if let Some(v) = call(client, "track.lyrics.get", &token, &[("track_id", tid)]).await {
            if let Some(l) = accept(body_text(&v, "lyrics/lyrics_body"), &share_url) {
                return Ok(l);
            }
        }
    }

    // Possibly an expired/blocked token — don't keep reusing it.
    invalidate_token();
    Err("Musixmatch: no lyrics found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_removes_commercial_use_footer() {
        let input = "Hello\nWorld\n\n******* This Lyrics is NOT for Commercial use *******\n(140961)";
        assert_eq!(clean_musixmatch_lyrics(input), "Hello\nWorld");
    }

    #[test]
    fn clean_only_trims_text_without_footer() {
        assert_eq!(clean_musixmatch_lyrics("  hello\nworld  \n"), "hello\nworld");
    }

    #[test]
    fn clean_keeps_genuine_asterisks_in_lyrics() {
        assert_eq!(
            clean_musixmatch_lyrics("Some lyrics\n*******\nmore lyrics"),
            "Some lyrics\n*******\nmore lyrics"
        );
    }

    #[test]
    fn clean_empty_input_returns_empty() {
        assert_eq!(clean_musixmatch_lyrics(""), "");
    }
}
