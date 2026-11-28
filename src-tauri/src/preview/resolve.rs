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
use crate::platforms::soundcloud::{self, availability, Availability, ScResource, ScTrack, ScTranscoding};
use crate::types::Platform;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

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
    /// Unix time (seconds) from which the url may be used. YouTube answers 403 to stream requests made
    /// earlier (yt-dlp: "Sleeping N seconds as required by the site").
    pub available_at: Option<f64>,
}

const SOUNDCLOUD_TTL: Duration = Duration::from_secs(4 * 60);
const BANDCAMP_TTL: Duration = Duration::from_secs(10 * 60);
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

/// Infers the actual host platform from `url` when the URL explicitly belongs to Bandcamp, SoundCloud, or YouTube.
pub fn platform_for_stream_url(fallback: Platform, url: &str) -> Platform {
    crate::platforms::platform_of_url(url).unwrap_or(fallback)
}

/// Cached or freshly resolved stream of the track at `url`.
pub async fn resolve(app: &AppHandle, platform: Platform, url: &str) -> Result<StreamInfo, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("empty track url".to_string());
    }
    let platform = platform_for_stream_url(platform, url);
    let slot = slot_for(&cache_key(platform, url));
    let mut guard = slot.lock().await;
    if let Some((info, at)) = guard.as_ref() {
        if at.elapsed() < ttl(platform) {
            return Ok(info.clone());
        }
    }
    let info = crate::platforms::stream(app, platform, url).await?;
    *guard = Some((info.clone(), Instant::now()));
    Ok(info)
}

/// Drops a cached url (for example after the server answered 403 because it expired).
pub async fn forget(platform: Platform, url: &str) {
    let platform = platform_for_stream_url(platform, url);
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
            available_at: None,
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
        .map_err(|e| format!("SoundCloud playlist request failed: {}", e.without_url()))?
        .error_for_status()
        .map_err(|e| format!("SoundCloud playlist request failed: {}", e.without_url()))?
        .text()
        .await
        .map_err(|e| format!("SoundCloud playlist could not be read: {}", e.without_url()))?;
    let playlist = parse_hls_playlist(&text, &location.url)?;
    if playlist.encrypted {
        return Err("this SoundCloud stream is encrypted".to_string());
    }
    Ok(StreamInfo {
        url: location.url,
        duration_ms,
        headers: Vec::new(),
        hls: Some(playlist),
        available_at: None,
    })
}

// ---------------------------------------------------------------------------------------------
// Bandcamp
// ---------------------------------------------------------------------------------------------

fn extract_tralbum_value(html: &str) -> Option<serde_json::Value> {
    for (prefix, quote) in [("data-tralbum=\"", '"'), ("data-tralbum='", '\'')] {
        if let Some(pos) = html.find(prefix) {
            let start = pos + prefix.len();
            if let Some(rel_end) = html[start..].find(quote) {
                let raw = &html[start..start + rel_end];
                let decoded = html_escape::decode_html_entities(raw);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&decoded) {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn normalize_bandcamp_stream_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("//") {
        Some(format!("https:{trimmed}"))
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// `(stream url, duration ms)` of the first streamable track in the `data-tralbum` blob of a Bandcamp page.
pub fn parse_bandcamp_stream(html: &str) -> Option<(String, Option<u64>)> {
    let value = extract_tralbum_value(html)?;
    let track = value.get("trackinfo")?.as_array()?.iter().find(|t| {
        t.get("file")
            .and_then(|f| f.as_object())
            .is_some_and(|f| f.values().any(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())))
    })?;
    let files = track.get("file")?.as_object()?;
    // Highest bitrate first; Bandcamp usually only publishes "mp3-128" for streaming.
    let raw_url = ["mp3-v0", "mp3-320", "mp3-128"]
        .iter()
        .find_map(|key| files.get(*key).and_then(|v| v.as_str()))
        .or_else(|| files.values().find_map(|v| v.as_str()))?;
    let url = normalize_bandcamp_stream_url(raw_url)?;
    let duration_ms = track
        .get("duration")
        .and_then(|d| d.as_f64())
        .map(|secs| (secs * 1000.0).round() as u64);
    Some((url, duration_ms))
}

pub(crate) async fn resolve_bandcamp(url: &str) -> Result<StreamInfo, String> {
    let html = crate::platforms::http::get_page(url).await?;
    let (stream_url, duration_ms) =
        parse_bandcamp_stream(&html).ok_or("no streamable audio found on the Bandcamp page")?;
    Ok(StreamInfo {
        url: stream_url,
        duration_ms,
        headers: Vec::new(),
        hls: None,
        available_at: None,
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

/// Fast client: tv_samsung (direct HTTPS audio without web_embedded JS challenge issues).
pub const YOUTUBE_FAST_CLIENTS: &str = "tv_samsung";
/// Fallback when the fast client does not offer the m4a stream: tv_samsung followed by web.
pub const YOUTUBE_FALLBACK_CLIENTS: &str = "tv_samsung,web";

/// yt-dlp arguments that print the duration, request headers and direct url of the AAC (m4a) audio stream
/// (symphonia decodes AAC in mp4 but not YouTube's webm/opus) WITHOUT the video url: the url is either
/// appended (`--`, url) or fed on stdin (`-a -`) to a process that was started in advance.
///
/// Web clients are used on purpose: their urls accept any byte range, while the urls of yt-dlp's default
/// client set (android_vr) answer 403 to every range but the first ~128 KiB.
pub fn youtube_args(clients: &str) -> Vec<String> {
    [
        "--no-playlist",
        "--no-warnings",
        "--no-check-formats",
        "--no-update",
        "--extractor-args",
        &format!("youtube:player_js_variant=tv;player-client={clients}"),
        "-f",
        "140/bestaudio[ext=m4a]",
        "--print",
        "available_at=%(available_at)s",
        "--print",
        "%(duration)s",
        "--print",
        "%(http_headers)j",
        "--print",
        "urls",
    ]
    .iter()
    .chain(crate::download::ytdlp::SYSTEM_CERTIFICATES.iter())
    .map(|s| s.to_string())
    .collect()
}

/// Parses the lines printed by `youtube_args`: duration in seconds, the request headers as JSON, then the url.
pub fn parse_youtube_output(output: &str) -> Option<StreamInfo> {
    let mut duration = None;
    let mut available_at = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut url = None;
    for line in output.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("http") {
            url = Some(line.to_string());
            break;
        } else if let Some(value) = line.strip_prefix("available_at=") {
            available_at = value.parse::<f64>().ok().filter(|t| t.is_finite() && *t > 0.0);
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
        available_at,
    })
}

/// How long to wait before the stream of `info` may be requested (zero when it is usable already).
pub fn wait_before_open(info: &StreamInfo, now_unix: f64) -> Duration {
    match info.available_at {
        Some(at) if at > now_unix => Duration::from_secs_f64((at - now_unix).min(30.0)),
        _ => Duration::ZERO,
    }
}

fn is_age_or_login_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("confirm your age")
        || lower.contains("login_required")
        || lower.contains("age-restricted")
        || lower.contains("use --cookies")
}

fn age_restricted_message(has_session: bool) -> &'static str {
    if has_session {
        "Age-restricted track — please reconnect your YouTube account in Settings → Accounts to listen to it."
    } else {
        "Age-restricted track — sign in to your YouTube account in Settings → Accounts to listen to it."
    }
}

fn youtube_cookie_args(app: &AppHandle) -> (bool, Vec<String>, Option<crate::download::cookies::CookieCopy>) {
    let Ok(profile) = crate::auth::youtube::profile_dir(app) else {
        return (false, Vec::new(), None);
    };
    let source = crate::download::cookies::find(&profile);
    if matches!(source, crate::download::cookies::CookieSource::None) {
        return (false, Vec::new(), None);
    }
    let copy_path = crate::download::cookies::copies_dir(
        &app.path().app_data_dir().unwrap_or_else(|_| std::env::temp_dir()),
    )
    .join(format!("preview_{}.txt", uuid::Uuid::new_v4().simple()));
    match source.args(&copy_path) {
        Ok((args, guard)) => (!args.is_empty(), args, guard),
        Err(_) => (false, Vec::new(), None),
    }
}

pub(crate) async fn resolve_youtube(app: &AppHandle, url: &str) -> Result<StreamInfo, String> {
    // Only a web address reaches YouTube or yt-dlp (also through the pre-started process's stdin).
    let url = crate::download::ytdlp::checked_url(url)?;
    // 1. Fast pure-HTTP InnerTube VISIONOS path (~90 ms warm, full byte-range 206 support, no ad wait).
    let mut visionos_err = None;
    if let Some(video_id) = extract_youtube_video_id(url) {
        match visionos::resolve(&video_id).await {
            Ok(info) => return Ok(info),
            Err(err) => {
                tracing::warn!(video_id = %video_id, error = %err, "VISIONOS YouTube resolve failed, falling back to yt-dlp");
                visionos_err = Some(err);
            }
        }
    }
    let (has_session, cookie_args, _cookie_guard) = youtube_cookie_args(app);
    if !has_session && visionos_err.as_deref().is_some_and(is_age_or_login_error) {
        return Err(age_restricted_message(false).to_string());
    }
    // 2. Fallback: use the connected YouTube session cookies when signed in; otherwise use the spare yt-dlp.
    let fast = if has_session {
        run_ytdlp_with_cookies(app, YOUTUBE_FAST_CLIENTS, url, &cookie_args).await
    } else {
        let res = match spare::take(app).await {
            Some(child) => spare::feed(child, url).await,
            None => run_ytdlp_with_cookies(app, YOUTUBE_FAST_CLIENTS, url, &[]).await,
        };
        spare::refill(app.clone());
        res
    };
    if let Some(info) = fast.as_deref().ok().and_then(parse_youtube_output) {
        return Ok(info);
    }
    // 3. The fast client sometimes does not offer the m4a stream: ask the fallback client set.
    let output = match run_ytdlp_with_cookies(app, YOUTUBE_FALLBACK_CLIENTS, url, &cookie_args).await {
        Ok(out) => out,
        Err(err) => {
            if is_age_or_login_error(&err) || fast.as_deref().err().is_some_and(|e| is_age_or_login_error(e)) {
                return Err(age_restricted_message(has_session).to_string());
            }
            return Err(err);
        }
    };
    parse_youtube_output(&output).ok_or_else(|| {
        let fast_err = fast.err().unwrap_or_default();
        if is_age_or_login_error(&fast_err) {
            age_restricted_message(has_session).to_string()
        } else {
            format!(
                "yt-dlp returned no audio stream{}",
                if fast_err.trim().is_empty() {
                    String::new()
                } else {
                    format!(" ({})", fast_err.trim())
                }
            )
        }
    })
}

/// Extracts the YouTube video ID from an HTTP(S) YouTube/YouTube Music/youtu.be URL.
pub fn extract_youtube_video_id(raw_url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(raw_url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let candidate = if host == "youtu.be" {
        parsed.path_segments()?.find(|s| !s.is_empty())?.to_string()
    } else if matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
        if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "v") {
            v.into_owned()
        } else {
            let mut segs = parsed.path_segments()?.filter(|s| !s.is_empty());
            match segs.next()? {
                "shorts" | "embed" | "live" => segs.next()?.to_string(),
                _ => return None,
            }
        }
    } else {
        return None;
    };
    if (10..=16).contains(&candidate.len())
        && candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Some(candidate)
    } else {
        None
    }
}

/// Fast pure-HTTP InnerTube VISIONOS extractor (no `yt-dlp` / `deno` subprocess, ~90 ms warm).
pub(crate) mod visionos {
    use super::StreamInfo;
    use serde::Deserialize;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    const CHROME_UA: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";
    const VISIONOS_UA: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
    const SESSION_TTL: Duration = Duration::from_secs(4 * 3600);
    const DEFAULT_STS: u64 = 20779;
    const MAX_WATCH_HTML_BYTES: usize = 8 * 1024 * 1024;
    const MAX_PLAYER_JSON_BYTES: usize = 4 * 1024 * 1024;

    #[derive(Clone)]
    struct Session {
        visitor_data: Option<String>,
        sts: u64,
        cookie_header: String,
        fetched_at: Instant,
    }

    fn http_client() -> Result<&'static reqwest::Client, String> {
        static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
        CLIENT
            .get_or_init(|| {
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .map_err(|e| format!("failed to build YouTube HTTP client: {e}"))
            })
            .as_ref()
            .map_err(|e| e.clone())
    }

    fn session_lock() -> &'static tokio::sync::Mutex<Option<Session>> {
        static SESSION: OnceLock<tokio::sync::Mutex<Option<Session>>> = OnceLock::new();
        SESSION.get_or_init(|| tokio::sync::Mutex::new(None))
    }

    async fn read_bounded(mut res: reqwest::Response, max_bytes: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(|e| format!("response read error: {e}"))? {
            if out.len().saturating_add(chunk.len()) > max_bytes {
                return Err("YouTube response exceeded size limit".to_string());
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    fn extract_json_str_field(haystack: &str, key: &str) -> Option<String> {
        let needle = format!("\"{key}\":\"");
        let start = haystack.find(&needle)? + needle.len();
        let rest = &haystack[start..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }

    fn extract_json_u64_field(haystack: &str, key: &str) -> Option<u64> {
        let needle = format!("\"{key}\":");
        let start = haystack.find(&needle)? + needle.len();
        let rest = haystack[start..].trim_start();
        let digits_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits_len == 0 {
            return None;
        }
        rest[..digits_len].parse::<u64>().ok()
    }

    async fn fetch_session(client: &reqwest::Client) -> Result<Session, String> {
        let res = client
            .get("https://www.youtube.com/watch?v=dQw4w9WgXcQ&bpctr=9999999999&has_verified=1")
            .header("User-Agent", CHROME_UA)
            .header("Accept-Language", "en-us,en;q=0.5")
            .header("Cookie", "PREF=hl=en&tz=UTC; SOCS=CAI")
            .send()
            .await
            .map_err(|_| "failed to fetch YouTube session".to_string())?;
        if !res.status().is_success() {
            return Err(format!("YouTube session request failed with HTTP {}", res.status()));
        }
        let mut cookies = vec!["PREF=hl=en&tz=UTC".to_string(), "SOCS=CAI".to_string()];
        for val in res.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(s) = val.to_str() {
                if let Some(pair) = s.split(';').next() {
                    let trimmed = pair.trim();
                    if !trimmed.is_empty() {
                        cookies.push(trimmed.to_string());
                    }
                }
            }
        }
        let body = read_bounded(res, MAX_WATCH_HTML_BYTES).await?;
        let html = String::from_utf8_lossy(&body);
        let visitor_data = extract_json_str_field(&html, "VISITOR_DATA");
        let sts = extract_json_u64_field(&html, "STS").unwrap_or(DEFAULT_STS);
        Ok(Session {
            visitor_data,
            sts,
            cookie_header: cookies.join("; "),
            fetched_at: Instant::now(),
        })
    }

    async fn get_session(client: &reqwest::Client, force_refresh: bool) -> Result<Session, String> {
        let mut guard = session_lock().lock().await;
        if !force_refresh {
            if let Some(existing) = guard.as_ref() {
                if existing.fetched_at.elapsed() < SESSION_TTL {
                    return Ok(existing.clone());
                }
            }
        }
        let fresh = fetch_session(client).await?;
        *guard = Some(fresh.clone());
        Ok(fresh)
    }

    pub async fn warm_up() {
        if let Ok(client) = http_client() {
            let _ = get_session(client, false).await;
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PlayerResponse {
        playability_status: Option<PlayabilityStatus>,
        streaming_data: Option<StreamingData>,
    }

    #[derive(Deserialize)]
    struct PlayabilityStatus {
        status: Option<String>,
        reason: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StreamingData {
        #[serde(default)]
        adaptive_formats: Vec<AdaptiveFormat>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AdaptiveFormat {
        itag: Option<u32>,
        url: Option<String>,
        mime_type: Option<String>,
        approx_duration_ms: Option<String>,
    }

    pub(crate) fn parse_player_response(body: &[u8]) -> Result<StreamInfo, (bool, String)> {
        let parsed: PlayerResponse = serde_json::from_slice(body)
            .map_err(|e| (false, format!("invalid YouTube player JSON: {e}")))?;
        let status = parsed
            .playability_status
            .as_ref()
            .and_then(|p| p.status.as_deref())
            .unwrap_or("");
        if status != "OK" {
            let reason = parsed
                .playability_status
                .as_ref()
                .and_then(|p| p.reason.as_deref())
                .unwrap_or("unplayable");
            let retryable = matches!(status, "UNPLAYABLE" | "LOGIN_REQUIRED");
            return Err((retryable, format!("YouTube player status {status}: {reason}")));
        }
        let formats = parsed
            .streaming_data
            .map(|s| s.adaptive_formats)
            .unwrap_or_default();
        let chosen = formats
            .iter()
            .find(|f| f.itag == Some(140) && f.url.as_deref().is_some_and(|u| u.starts_with("https://")))
            .or_else(|| {
                formats.iter().find(|f| {
                    f.mime_type
                        .as_deref()
                        .is_some_and(|m| m.starts_with("audio/mp4"))
                        && f.url.as_deref().is_some_and(|u| u.starts_with("https://"))
                })
            })
            .ok_or_else(|| (false, "no direct audio/mp4 stream in VISIONOS response".to_string()))?;
        let url = chosen.url.clone().ok_or_else(|| (false, "missing stream url".to_string()))?;
        let duration_ms = chosen
            .approx_duration_ms
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok());
        Ok(StreamInfo {
            url,
            duration_ms,
            headers: vec![("User-Agent".to_string(), CHROME_UA.to_string())],
            hls: None,
            available_at: None,
        })
    }

    async fn request_player(
        client: &reqwest::Client,
        session: &Session,
        video_id: &str,
    ) -> Result<Vec<u8>, String> {
        let payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "VISIONOS",
                    "clientVersion": "1.02",
                    "deviceMake": "Apple",
                    "deviceModel": "RealityDevice17,1",
                    "userAgent": VISIONOS_UA,
                    "osName": "visionOS",
                    "osVersion": "26.5.23O471",
                    "hl": "en",
                    "timeZone": "UTC",
                    "utcOffsetMinutes": 0
                }
            },
            "videoId": video_id,
            "playbackContext": {
                "contentPlaybackContext": {
                    "html5Preference": "HTML5_PREF_WANTS",
                    "signatureTimestamp": session.sts
                }
            },
            "contentCheckOk": true,
            "racyCheckOk": true
        });
        let mut req = client
            .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("Content-Type", "application/json")
            .header("User-Agent", VISIONOS_UA)
            .header("X-Youtube-Client-Name", "101")
            .header("X-Youtube-Client-Version", "1.02")
            .header("Origin", "https://www.youtube.com")
            .header("Cookie", &session.cookie_header)
            .json(&payload);
        if let Some(visitor) = session.visitor_data.as_deref() {
            req = req.header("X-Goog-Visitor-Id", visitor);
        }
        let res = req
            .send()
            .await
            .map_err(|_| "YouTube player API request failed".to_string())?;
        if !res.status().is_success() {
            return Err(format!("YouTube player API HTTP {}", res.status()));
        }
        read_bounded(res, MAX_PLAYER_JSON_BYTES).await
    }

    pub async fn resolve(video_id: &str) -> Result<StreamInfo, String> {
        let client = http_client()?;
        let session = get_session(client, false).await?;
        let body = request_player(client, &session, video_id).await?;
        match parse_player_response(&body) {
            Ok(info) => Ok(info),
            Err((true, _)) => {
                let refreshed = get_session(client, true).await?;
                let retry_body = request_player(client, &refreshed, video_id).await?;
                parse_player_response(&retry_body).map_err(|(_, msg)| msg)
            }
            Err((false, msg)) => Err(msg),
        }
    }
}

/// Warms YouTube's session in the background at app start so the first stream skips session setup.
pub fn warm_up(_app: AppHandle) {
    tauri::async_runtime::spawn(visionos::warm_up());
}

/// Replaces the waiting process with one of the yt-dlp now on disk (after an update).
pub fn restart_spare(app: AppHandle) {
    spare::restart(app);
}

async fn run_ytdlp_with_cookies(
    app: &AppHandle,
    clients: &str,
    url: &str,
    cookie_args: &[String],
) -> Result<String, String> {
    let mut args = youtube_args(clients);
    args.extend(cookie_args.iter().cloned());
    args.push("--".to_string());
    args.push(url.to_string());
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::download::ytdlp::execute(app, &arg_refs).await
}

/// One yt-dlp process started ahead of time with `-a -`: it loads Python and the extractors, then waits
/// for the video url on stdin. Writing the url and closing stdin saves ~1 s per YouTube stream.
mod spare {
    use super::{youtube_args, YOUTUBE_FAST_CLIENTS};
    use std::process::Stdio;
    use std::sync::OnceLock;
    use tauri::AppHandle;
    use tokio::process::{Child, Command};

    fn slot() -> &'static tokio::sync::Mutex<Option<Child>> {
        static SLOT: OnceLock<tokio::sync::Mutex<Option<Child>>> = OnceLock::new();
        SLOT.get_or_init(|| tokio::sync::Mutex::new(None))
    }

    fn spawn(app: &AppHandle) -> Option<Child> {
        let binary = crate::download::ytdlp::get_path(app);
        if !binary.exists() {
            return None;
        }
        let mut cmd = Command::new(binary);
        crate::download::ytdlp::prepare_command(&mut cmd, app);
        cmd.args(youtube_args(YOUTUBE_FAST_CLIENTS))
            .args(["-a", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd.spawn().ok()
    }

    /// The waiting process, if one is ready (a process that already exited is discarded).
    pub async fn take(app: &AppHandle) -> Option<Child> {
        let mut slot = slot().lock().await;
        let mut child = slot.take()?;
        drop(slot);
        match child.try_wait() {
            Ok(None) => Some(child),
            _ => {
                refill(app.clone());
                None
            }
        }
    }

    /// Ends the waiting process (killed when dropped) and starts a new one.
    pub fn restart(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut slot = slot().lock().await;
            *slot = None;
            *slot = spawn(&app);
        });
    }

    /// Makes sure a process is waiting for the next url.
    pub fn refill(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut slot = slot().lock().await;
            let alive = match slot.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(None)),
                None => false,
            };
            if !alive {
                *slot = spawn(&app);
            }
        });
    }

    /// Hands `url` to a waiting process and returns its stdout.
    pub async fn feed(child: Child, url: &str) -> Result<String, String> {
        let output = crate::download::ytdlp::wait_with_deadline(
            child,
            Some(format!("{url}\n").into_bytes()),
            std::time::Duration::from_secs(60),
        ).await?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
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
    fn bandcamp_handles_single_quotes_and_protocol_relative_urls() {
        let html = r#"<div data-tralbum='{"trackinfo":[{"duration":90.5,"file":{"mp3-128":"//t4.bcbits.com/stream/abc"}}]}'></div>"#;
        let (url, duration) = parse_bandcamp_stream(html).expect("stream");
        assert_eq!(url, "https://t4.bcbits.com/stream/abc");
        assert_eq!(duration, Some(90_500));
    }

    #[test]
    fn platform_for_stream_url_routes_bandcamp_and_soundcloud_domains() {
        assert_eq!(
            platform_for_stream_url(Platform::Youtube, "https://heavenpierceher.bandcamp.com/track/the-fall"),
            Platform::Bandcamp
        );
        assert_eq!(
            platform_for_stream_url(Platform::Youtube, "https://soundcloud.com/a/b"),
            Platform::Soundcloud
        );
        assert_eq!(
            platform_for_stream_url(Platform::Bandcamp, "https://music.youtube.com/watch?v=123"),
            Platform::Youtube
        );
    }

    #[test]
    fn youtube_args_select_clients_and_leave_the_url_out() {
        let args = youtube_args(YOUTUBE_FAST_CLIENTS);
        assert!(args.iter().any(|a| a == "youtube:player_js_variant=tv;player-client=tv_samsung"));
        assert!(args.iter().any(|a| a == "140/bestaudio[ext=m4a]"));
        assert!(!args.iter().any(|a| a == "--" || a.starts_with("http")));
        let fallback = youtube_args(YOUTUBE_FALLBACK_CLIENTS);
        assert!(fallback.iter().any(|a| a == "youtube:player_js_variant=tv;player-client=tv_samsung,web"));
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
        let timed = parse_youtube_output("available_at=1794229978
215
https://rr1.googlevideo.com/v").unwrap();
        assert_eq!(timed.available_at, Some(1_794_229_978.0));
        assert_eq!(timed.duration_ms, Some(215_000));
        assert_eq!(parse_youtube_output("available_at=NA
https://r/v").unwrap().available_at, None);
        let no_duration = parse_youtube_output("https://rr1.googlevideo.com/v").unwrap();
        assert_eq!(no_duration.duration_ms, None);
        assert_eq!(parse_youtube_output("NA\nhttps://rr/v").unwrap().duration_ms, None);
    }

    #[test]
    fn wait_before_open_honours_available_at() {
        let mut info = parse_youtube_output("https://r/v").unwrap();
        assert_eq!(wait_before_open(&info, 100.0), Duration::ZERO);
        info.available_at = Some(102.5);
        assert_eq!(wait_before_open(&info, 100.0), Duration::from_millis(2500));
        assert_eq!(wait_before_open(&info, 103.0), Duration::ZERO);
        info.available_at = Some(1000.0);
        assert_eq!(wait_before_open(&info, 100.0), Duration::from_secs(30), "capped");
    }

    #[test]
    fn youtube_output_garbage_is_none() {
        assert!(parse_youtube_output("").is_none());
        assert!(parse_youtube_output("215").is_none());
        assert!(parse_youtube_output("215\nnot a url").is_none());
    }

    #[test]
    fn extracts_youtube_video_id_from_allowed_hosts_only() {
        assert_eq!(
            extract_youtube_video_id("https://music.youtube.com/watch?v=xdIGp4Ge8yk&list=RD"),
            Some("xdIGp4Ge8yk".to_string())
        );
        assert_eq!(
            extract_youtube_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            extract_youtube_video_id("https://youtu.be/FTQbiNvZqaY?t=10"),
            Some("FTQbiNvZqaY".to_string())
        );
        assert_eq!(
            extract_youtube_video_id("https://www.youtube.com/shorts/8oCoGvy6RK4"),
            Some("8oCoGvy6RK4".to_string())
        );
        assert_eq!(
            extract_youtube_video_id("https://evil.com/watch?v=xdIGp4Ge8yk"),
            None
        );
        assert_eq!(
            extract_youtube_video_id("https://evil.com/youtube.com/watch?v=xdIGp4Ge8yk"),
            None
        );
        assert_eq!(
            extract_youtube_video_id("file:///C:/watch?v=xdIGp4Ge8yk"),
            None
        );
    }

    #[test]
    fn visionos_player_parser_selects_140_and_flags_retryable_errors() {
        let ok_json = br#"{
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "adaptiveFormats": [
                    {"itag": 139, "url": "https://rr1.googlevideo.com/low", "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "approxDurationMs": "231000"},
                    {"itag": 140, "url": "https://rr1.googlevideo.com/high", "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "approxDurationMs": "231000"}
                ]
            }
        }"#;
        let info = visionos::parse_player_response(ok_json).expect("valid player json");
        assert_eq!(info.url, "https://rr1.googlevideo.com/high");
        assert_eq!(info.duration_ms, Some(231_000));
        assert_eq!(info.available_at, None);

        let bot_json = br#"{
            "playabilityStatus": {"status": "UNPLAYABLE", "reason": "Sign in to confirm you're not a bot"}
        }"#;
        let err = visionos::parse_player_response(bot_json).unwrap_err();
        assert!(err.0, "UNPLAYABLE should trigger one session refresh retry");
    }

    #[test]
    fn age_gate_errors_suggest_signing_in_or_reconnecting() {
        assert!(is_age_or_login_error("YouTube player status LOGIN_REQUIRED: Sign in to confirm your age"));
        assert!(is_age_or_login_error("ERROR: [youtube] LBs6fU8s8vw: Sign in to confirm your age. Use --cookies-from-browser"));
        assert!(!is_age_or_login_error("Connection timed out"));
        assert!(age_restricted_message(false).contains("sign in to your YouTube account"));
        assert!(age_restricted_message(true).contains("reconnect your YouTube account"));
    }
}
