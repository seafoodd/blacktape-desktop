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
    /// Unix time (seconds) from which the url may be used. YouTube answers 403 to stream requests made
    /// earlier (yt-dlp: "Sleeping N seconds as required by the site").
    pub available_at: Option<f64>,
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
        available_at: None,
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

/// Fast client: the embedded web player alone (~1.3 s less work than the downloader's three clients).
pub const YOUTUBE_FAST_CLIENTS: &str = "web_embedded";
/// Fallback when the fast client does not offer the m4a stream: the downloader's client set.
pub const YOUTUBE_FALLBACK_CLIENTS: &str = "web_embedded,web,tv";

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
        &format!("youtube:player-client={clients}"),
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

async fn resolve_youtube(app: &AppHandle, url: &str) -> Result<StreamInfo, String> {
    // Only a web address reaches yt-dlp (also through the pre-started process's stdin).
    let url = crate::download::ytdlp::checked_url(url)?;
    // 1. Fast path: the pre-started yt-dlp process (interpreter already loaded) with the fast client.
    let fast = match spare::take(app).await {
        Some(child) => spare::feed(child, url).await,
        None => run_ytdlp(app, YOUTUBE_FAST_CLIENTS, url).await,
    };
    spare::refill(app.clone());
    if let Some(info) = fast.as_deref().ok().and_then(parse_youtube_output) {
        return Ok(info);
    }
    // 2. The fast client sometimes does not offer the m4a stream: ask the downloader's client set.
    let output = run_ytdlp(app, YOUTUBE_FALLBACK_CLIENTS, url).await?;
    parse_youtube_output(&output).ok_or_else(|| {
        format!(
            "yt-dlp returned no audio stream{}",
            fast.err().map(|e| format!(" ({})", e.trim())).unwrap_or_default()
        )
    })
}

/// Starts yt-dlp in the background once (e.g. at app start) so the first YouTube stream skips the
/// interpreter start-up.
pub fn warm_up(app: AppHandle) {
    spare::refill(app);
}

async fn run_ytdlp(app: &AppHandle, clients: &str, url: &str) -> Result<String, String> {
    let mut args = youtube_args(clients);
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
    use tokio::io::AsyncWriteExt;
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
    pub async fn feed(mut child: Child, url: &str) -> Result<String, String> {
        let mut stdin = child.stdin.take().ok_or("yt-dlp stdin unavailable")?;
        stdin
            .write_all(format!("{url}
").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        drop(stdin); // EOF: yt-dlp starts working
        let output = child.wait_with_output().await.map_err(|e| e.to_string())?;
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
    fn youtube_args_select_clients_and_leave_the_url_out() {
        let args = youtube_args(YOUTUBE_FAST_CLIENTS);
        assert!(args.iter().any(|a| a == "youtube:player-client=web_embedded"));
        assert!(args.iter().any(|a| a == "140/bestaudio[ext=m4a]"));
        assert!(!args.iter().any(|a| a == "--" || a.starts_with("http")));
        let fallback = youtube_args(YOUTUBE_FALLBACK_CLIENTS);
        assert!(fallback.iter().any(|a| a == "youtube:player-client=web_embedded,web,tv"));
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
}
