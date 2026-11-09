//! Resolves the page url of a track to a direct audio url that can be streamed with HTTP range requests.
//!
//! * SoundCloud: api-v2 track -> progressive (plain HTTP mp3) transcoding -> signed media url. Two requests.
//! * Bandcamp: the track/album page carries the `mp3-128` url in `data-tralbum`. One request.
//! * YouTube: `yt-dlp -g` for the AAC (m4a) audio stream (the only slow one, ~3 s), so results are cached
//!   and the frontend can warm the cache while the user hovers a track.
//!
//! Resolved urls are cached per (platform, url) for a platform specific time; concurrent requests for the
//! same track wait for one lookup instead of running it twice.

use crate::preview::segmented_source::{parse_hls_playlist, HlsPlaylist};
use crate::soundcloud::{self, availability, Availability, ScResource, ScTrack, ScTranscoding};
use crate::types::Platform;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// A direct, streamable audio url.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamInfo {
    pub url: String,
    /// Length in milliseconds when the platform told us.
    pub duration_ms: Option<u64>,
    /// Request headers the media server expects (YouTube ties urls to the client that produced them).
    pub headers: Vec<(String, String)>,
    /// Set for HLS streams (SoundCloud): `url` is then the playlist and the parts are played concatenated.
    pub hls: Option<HlsPlaylist>,
}

const SOUNDCLOUD_TTL: Duration = Duration::from_secs(4 * 60);
const BANDCAMP_TTL: Duration = Duration::from_secs(30 * 60);
const YOUTUBE_TTL: Duration = Duration::from_secs(3 * 60 * 60);

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";

/// How long a resolved url of `platform` stays valid in the cache.
pub fn ttl(platform: Platform) -> Duration {
    match platform {
        Platform::Soundcloud => SOUNDCLOUD_TTL,
        Platform::Bandcamp => BANDCAMP_TTL,
        Platform::Youtube => YOUTUBE_TTL,
        Platform::Local => Duration::ZERO,
    }
}

type Slot = Arc<tokio::sync::Mutex<Option<(StreamInfo, Instant)>>>;

fn slots() -> &'static Mutex<HashMap<String, Slot>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(platform: Platform, url: &str) -> String {
    format!("{platform:?}|{}", url.trim())
}

fn slot_for(key: &str) -> Slot {
    let mut map = slots().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > 256 {
        // Forget idle slots (nobody else holds them); cheap housekeeping.
        map.retain(|_, slot| Arc::strong_count(slot) > 1);
    }
    map.entry(key.to_string()).or_default().clone()
}

/// Cached or freshly resolved stream of the track at `url`.
pub async fn resolve(app: &AppHandle, platform: Platform, url: &str) -> Result<StreamInfo, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("empty track url".to_string());
    }
    let slot = slot_for(&cache_key(platform, url));
    let mut guard = slot.lock().await;
    if let Some((info, at)) = guard.as_ref() {
        if at.elapsed() < ttl(platform) {
            return Ok(info.clone());
        }
    }
    let info = match platform {
        Platform::Soundcloud => resolve_soundcloud(url).await?,
        Platform::Bandcamp => resolve_bandcamp(url).await?,
        Platform::Youtube => resolve_youtube(app, url).await?,
        Platform::Local => return Err("local files are already playable".to_string()),
    };
    *guard = Some((info.clone(), Instant::now()));
    Ok(info)
}

/// Drops a cached url (for example after the server answered 403 because it expired).
pub async fn forget(platform: Platform, url: &str) {
    let slot = slot_for(&cache_key(platform, url));
    *slot.lock().await = None;
}

// ---------------------------------------------------------------------------------------------
// SoundCloud
// ---------------------------------------------------------------------------------------------

/// The plain HTTP (progressive) full-length stream of a track, if it has one. Preview (snipped) and
/// HLS / encrypted streams are not usable for the streaming source.
/// The HLS stream that can be played by concatenating its fragments: unencrypted fragmented-mp4 AAC
/// (`audio/mp4`), highest bitrate first. Adaptive master playlists, opus and encrypted streams are skipped.
pub fn hls_transcoding(track: &ScTrack) -> Option<&ScTranscoding> {
    track
        .media
        .as_ref()?
        .transcodings
        .iter()
        .filter(|t| {
            t.format.protocol == "hls"
                && t.format.mime_type.contains("audio/mp4")
                && !t.snipped
                && !t.url.is_empty()
                && !t.url.contains("/preview/")
        })
        .max_by_key(|t| hls_bitrate(&t.preset))
}

/// `"aac_160k"` -> 160.
fn hls_bitrate(preset: &str) -> u32 {
    preset
        .rsplit('_')
        .next()
        .and_then(|part| part.strip_suffix('k'))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

pub fn progressive_transcoding(track: &ScTrack) -> Option<&ScTranscoding> {
    track
        .media
        .as_ref()?
        .transcodings
        .iter()
        .filter(|t| {
            t.format.protocol == "progressive"
                && !t.snipped
                && !t.url.is_empty()
                && !t.url.contains("/preview/")
        })
        .max_by_key(|t| u8::from(t.preset.starts_with("mp3")))
}

#[derive(Deserialize)]
struct MediaLocation {
    url: String,
}

pub(crate) async fn resolve_soundcloud(url: &str) -> Result<StreamInfo, String> {
    let track = match soundcloud::resolve(url).await? {
        ScResource::Track(track) => track,
        _ => return Err("not a SoundCloud track".to_string()),
    };
    match availability(&track) {
        Availability::Full => {}
        other => {
            return Err(other
                .reason()
                .unwrap_or_else(|| "track can't be streamed".to_string()))
        }
    }
    let mut params: Vec<(&str, String)> = Vec::new();
    if let Some(token) = track.track_authorization.as_deref().filter(|t| !t.is_empty()) {
        params.push(("track_authorization", token.to_string()));
    }
    let duration_ms = track.full_duration.or(track.duration);
    if let Some(transcoding) = progressive_transcoding(&track) {
        let location: MediaLocation = soundcloud::api_get(&transcoding.url, &params).await?;
        return Ok(StreamInfo {
            url: location.url,
            duration_ms,
            headers: Vec::new(),
            hls: None,
        });
    }
    // Most tracks are HLS only: fetch the playlist so the fragments can be played back to back.
    let transcoding = hls_transcoding(&track)
        .ok_or_else(|| "this SoundCloud track has no stream that can be played".to_string())?;
    let location: MediaLocation = soundcloud::api_get(&transcoding.url, &params).await?;
    let text = http_client()
        .get(&location.url)
        .send()
        .await
        .map_err(|e| format!("SoundCloud playlist request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("SoundCloud playlist request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("SoundCloud playlist could not be read: {e}"))?;
    let playlist = parse_hls_playlist(&text, &location.url)?;
    if playlist.encrypted {
        return Err("this SoundCloud stream is encrypted".to_string());
    }
    Ok(StreamInfo {
        url: location.url,
        duration_ms,
        headers: Vec::new(),
        hls: Some(playlist),
    })
}

// ---------------------------------------------------------------------------------------------
// Bandcamp
// ---------------------------------------------------------------------------------------------

/// `(stream url, duration ms)` of the first track in the `data-tralbum` blob of a Bandcamp page.
pub fn parse_bandcamp_stream(html: &str) -> Option<(String, Option<u64>)> {
    let start = html.find("data-tralbum=\"")? + "data-tralbum=\"".len();
    let end = html[start..].find('"')? + start;
    let json = html_escape::decode_html_entities(&html[start..end]);
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let track = value.get("trackinfo")?.as_array()?.iter().find(|t| {
        t.get("file")
            .and_then(|f| f.as_object())
            .is_some_and(|f| !f.is_empty())
    })?;
    let files = track.get("file")?.as_object()?;
    // Highest bitrate first; Bandcamp usually only publishes "mp3-128" for streaming.
    let url = ["mp3-v0", "mp3-320", "mp3-128"]
        .iter()
        .find_map(|key| files.get(*key).and_then(|v| v.as_str()))
        .or_else(|| files.values().find_map(|v| v.as_str()))?;
    let duration_ms = track
        .get("duration")
        .and_then(|d| d.as_f64())
        .map(|secs| (secs * 1000.0).round() as u64);
    Some((url.to_string(), duration_ms))
}

pub(crate) async fn resolve_bandcamp(url: &str) -> Result<StreamInfo, String> {
    let host_ok = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .is_some_and(|h| h == "bandcamp.com" || h.ends_with(".bandcamp.com"));
    if !host_ok {
        return Err("not a Bandcamp url".to_string());
    }
    let html = http_client()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Bandcamp request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Bandcamp request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Bandcamp page could not be read: {e}"))?;
    let (url, duration_ms) =
        parse_bandcamp_stream(&html).ok_or("no streamable audio found on the Bandcamp page")?;
    Ok(StreamInfo {
        url,
        duration_ms,
        headers: Vec::new(),
        hls: None,
    })
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default()
    })
}

// ---------------------------------------------------------------------------------------------
// YouTube
// ---------------------------------------------------------------------------------------------

/// yt-dlp arguments that print the direct url of the AAC (m4a) audio stream (symphonia decodes AAC in
/// mp4 but not YouTube's webm/opus). The downloader's client set (`web_embedded,web,tv`) is used on purpose:
/// its urls accept any byte range, while the urls of yt-dlp's default client set (android_vr) answer 403 to
/// every range except the first ~128 KiB, which makes seeking and long tracks impossible.
pub fn youtube_args(url: &str) -> Vec<String> {
    [
        "--no-playlist",
        "--no-warnings",
        "--no-check-formats",
        "--extractor-args",
        "youtube:player-client=web_embedded,web,tv",
        "-f",
        "140/bestaudio[ext=m4a]",
        "--print",
        "%(duration)s",
        "--print",
        "%(http_headers)j",
        "--print",
        "urls",
        "--",
        url,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Parses the lines printed by `youtube_args`: duration in seconds, the request headers as JSON, then the url.
pub fn parse_youtube_output(output: &str) -> Option<StreamInfo> {
    let mut duration = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut url = None;
    for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("http") {
            url = Some(line.to_string());
            break;
        } else if line.starts_with('{') {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(line) {
                headers = map.into_iter().collect();
                headers.sort();
            }
        } else if let Ok(secs) = line.parse::<f64>() {
            duration = Some(secs);
        }
    }
    Some(StreamInfo {
        url: url?,
        duration_ms: duration.map(|secs| (secs * 1000.0).round() as u64),
        headers,
        hls: None,
    })
}

async fn resolve_youtube(app: &AppHandle, url: &str) -> Result<StreamInfo, String> {
    let args = youtube_args(url);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // YouTube intermittently does not offer the m4a stream to these clients ("format is not available"),
    // a second try usually succeeds.
    let mut last_error = String::new();
    for _ in 0..2 {
        match crate::download::ytdlp::execute(app, &arg_refs).await {
            Ok(output) => {
                if let Some(info) = parse_youtube_output(&output) {
                    return Ok(info);
                }
                last_error = "yt-dlp returned no audio stream".to_string();
            }
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_per_platform() {
        assert!(ttl(Platform::Soundcloud) < ttl(Platform::Bandcamp));
        assert!(ttl(Platform::Bandcamp) < ttl(Platform::Youtube));
        assert_eq!(ttl(Platform::Local), Duration::ZERO);
    }

    #[test]
    fn cache_key_separates_platforms_and_trims() {
        assert_ne!(
            cache_key(Platform::Youtube, "https://x"),
            cache_key(Platform::Soundcloud, "https://x")
        );
        assert_eq!(
            cache_key(Platform::Youtube, " https://x "),
            cache_key(Platform::Youtube, "https://x")
        );
    }

    #[test]
    fn bandcamp_stream_is_read_from_tralbum() {
        let html = r#"<div data-tralbum="{&quot;trackinfo&quot;:[{&quot;title&quot;:&quot;a&quot;,&quot;duration&quot;:111.644,&quot;file&quot;:{&quot;mp3-128&quot;:&quot;https://t4.bcbits.com/stream/x/mp3-128/1?p=0&amp;ts=1&quot;}}]}"></div>"#;
        let (url, duration) = parse_bandcamp_stream(html).expect("stream");
        assert_eq!(url, "https://t4.bcbits.com/stream/x/mp3-128/1?p=0&ts=1");
        assert_eq!(duration, Some(111_644));
    }

    #[test]
    fn bandcamp_skips_unstreamable_tracks_and_prefers_best_bitrate() {
        let html = r#"data-tralbum="{&quot;trackinfo&quot;:[{&quot;file&quot;:null},{&quot;duration&quot;:1,&quot;file&quot;:{&quot;mp3-128&quot;:&quot;https://a/128&quot;,&quot;mp3-v0&quot;:&quot;https://a/v0&quot;}}]}""#;
        let (url, _) = parse_bandcamp_stream(html).expect("stream");
        assert_eq!(url, "https://a/v0");
    }

    #[test]
    fn bandcamp_garbage_is_none() {
        assert!(parse_bandcamp_stream("").is_none());
        assert!(parse_bandcamp_stream("data-tralbum=\"not json\"").is_none());
        assert!(parse_bandcamp_stream("data-tralbum=\"{&quot;trackinfo&quot;:[]}\"").is_none());
    }

    #[test]
    fn youtube_args_end_with_the_url() {
        let args = youtube_args("https://www.youtube.com/watch?v=abc");
        assert_eq!(args.last().map(String::as_str), Some("https://www.youtube.com/watch?v=abc"));
        assert!(args.iter().any(|a| a == "140/bestaudio[ext=m4a]"));
        assert!(args.iter().any(|a| a == "youtube:player-client=web_embedded,web,tv"));
        assert_eq!(args[args.len() - 2], "--");
    }

    #[test]
    fn youtube_output_is_duration_then_url() {
        let info = parse_youtube_output("215\nhttps://rr1.googlevideo.com/videoplayback?x=1\n").unwrap();
        assert_eq!(info.duration_ms, Some(215_000));
        assert!(info.url.starts_with("https://rr1."));
        let with_headers = parse_youtube_output("215
{\"User-Agent\": \"UA\", \"Accept\": \"*/*\"}
https://rr1.googlevideo.com/v").unwrap();
        assert_eq!(
            with_headers.headers,
            vec![("Accept".to_string(), "*/*".to_string()), ("User-Agent".to_string(), "UA".to_string())]
        );
        let no_duration = parse_youtube_output("https://rr1.googlevideo.com/v").unwrap();
        assert_eq!(no_duration.duration_ms, None);
        assert_eq!(parse_youtube_output("NA\nhttps://rr/v").unwrap().duration_ms, None);
    }

    #[test]
    fn youtube_output_garbage_is_none() {
        assert!(parse_youtube_output("").is_none());
        assert!(parse_youtube_output("215").is_none());
        assert!(parse_youtube_output("215\nnot a url").is_none());
    }
}
