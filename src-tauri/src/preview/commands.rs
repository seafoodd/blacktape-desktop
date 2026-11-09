//! Streaming playback: `open_stream` resolves a track page to a direct audio url, opens it (HTTP or HLS) and
//! plays it through the normal audio player (`AudioPlayer::play_stream`), so the existing player controls,
//! queue handling, media keys and volume all apply. `stream_prefetch` warms the url cache.

use crate::audio::player::{AudioPlayer, BoxedSource};
use crate::preview::http_source::HttpSource;
use crate::preview::resolve::{self, StreamInfo};
use crate::preview::segmented_source::SegmentedSource;
use crate::types::{Platform, QualityTier, Song};
use rodio::{Decoder, Source};
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{command, AppHandle, State};

/// What the frontend sends to start a stream (the metadata becomes the "current song" of the player).
#[derive(Debug, Clone, Deserialize)]
pub struct StreamRequest {
    pub platform: Platform,
    pub url: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub cover: Option<String>,
    /// Duration known from the search result, used until the decoder reports its own.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Warm-ups run at most this many at a time (each one spawns yt-dlp for YouTube).
const PREFETCH_CONCURRENCY: usize = 2;

fn prefetch_slots() -> &'static tokio::sync::Semaphore {
    static SLOTS: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| tokio::sync::Semaphore::new(PREFETCH_CONCURRENCY))
}

/// The pseudo song shown by the player while streaming (`id` None: it is not in the library).
pub fn stream_song(request: &StreamRequest, duration_ms: Option<u64>) -> Song {
    Song {
        id: None,
        path: String::new(),
        title: request.title.clone(),
        artists: vec![request.artist.clone()],
        album_artist: request.artist.clone(),
        album: String::new(),
        duration_ms: duration_ms.unwrap_or(0),
        track_number: None,
        genre: None,
        release_year: None,
        cover_url: request.cover.clone().filter(|c| !c.is_empty()),
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
        source: request.platform,
        source_url: Some(request.url.clone()),
        source_item_id: None,
        canonical_track_slug: String::new(),
        canonical_album_slug: String::new(),
        quality_tier: QualityTier::Standard,
    }
}

fn blocking_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36",
            )
            .connect_timeout(Duration::from_secs(5))
            .tcp_nodelay(true)
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default()
    })
}

/// A client that sends `headers` with every request (urls that are bound to the client that produced them).
fn client_with_headers(headers: &[(String, String)]) -> Result<reqwest::blocking::Client, String> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
    let mut map = HeaderMap::new();
    for (name, value) in headers {
        // Range / encoding headers are managed by the source itself.
        if matches!(name.to_ascii_lowercase().as_str(), "range" | "accept-encoding" | "host" | "content-length") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
            map.insert(name, value);
        }
    }
    reqwest::blocking::Client::builder()
        .default_headers(map)
        .connect_timeout(Duration::from_secs(5))
        .tcp_nodelay(true)
        .build()
        .map_err(|e| e.to_string())
}

/// File extension hint for the decoder from the stream url / content type.
fn format_hint(url: &str, content_type: Option<&str>) -> Option<&'static str> {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("mpeg") || ct.contains("mp3") {
        return Some("mp3");
    }
    if ct.contains("mp4") || ct.contains("m4a") || ct.contains("aac") {
        return Some("m4a");
    }
    let lower = url.to_ascii_lowercase();
    if lower.contains("mime=audio%2fmp4") || lower.contains("mime=audio/mp4") {
        return Some("m4a");
    }
    if lower.contains("/mp3") || lower.contains(".mp3") {
        return Some("mp3");
    }
    None
}

/// Opens the stream and builds the decoder. Blocking (runs the first network round trip).
fn open_decoder(info: &StreamInfo) -> Result<(BoxedSource, Option<Duration>), String> {
    let url = info.url.as_str();
    let client = if info.headers.is_empty() {
        blocking_client().clone()
    } else {
        client_with_headers(&info.headers)?
    };
    if let Some(playlist) = &info.hls {
        // HLS (SoundCloud): fragmented mp4, played as the concatenation of init + fragments.
        let source = SegmentedSource::open(client, playlist.init.clone(), playlist.segments.clone())
            .map_err(|e| e.to_string())?;
        let len = source.content_length();
        let decoder = Decoder::builder()
            .with_data(source)
            .with_seekable(true)
            .with_byte_len(len)
            .with_hint("m4a")
            .build()
            .map_err(|e| e.to_string())?;
        let total = decoder.total_duration();
        return Ok((Box::new(decoder), total));
    }
    // googlevideo rejects open-ended ranges: go straight to bounded chunks and save the failing round trip.
    let source = if url.contains(".googlevideo.com/") {
        HttpSource::open_chunked(client, url)
    } else {
        HttpSource::open(client, url)
    }
    .map_err(|e| e.to_string())?;
    let len = source.content_length();
    let hint = format_hint(url, source.content_type());
    let mut builder = Decoder::builder().with_data(source).with_seekable(true);
    if let Some(len) = len {
        builder = builder.with_byte_len(len);
    }
    if let Some(hint) = hint {
        builder = builder.with_hint(hint);
    }
    let decoder = builder.build().map_err(|e| e.to_string())?;
    let total = decoder.total_duration();
    Ok((Box::new(decoder), total))
}

/// Sleeps until the url of `info` may be used (YouTube); returns at once for everything else.
async fn wait_until_available(info: &StreamInfo) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let wait = resolve::wait_before_open(info, now);
    if !wait.is_zero() {
        tokio::time::sleep(wait + Duration::from_millis(100)).await;
    }
}

async fn open_with_retry(
    app: &AppHandle,
    platform: Platform,
    url: &str,
    info: StreamInfo,
) -> Result<(BoxedSource, Option<Duration>, StreamInfo), String> {
    wait_until_available(&info).await;
    let first = info.clone();
    match tokio::task::spawn_blocking(move || open_decoder(&first))
        .await
        .map_err(|e| e.to_string())?
    {
        Ok((source, total)) => Ok((source, total, info)),
        Err(first_error) => {
            // The cached url may have expired: look the track up again once.
            eprintln!("[preview] open failed ({first_error}), re-resolving");
            resolve::forget(platform, url).await;
            let fresh = resolve::resolve(app, platform, url).await?;
            wait_until_available(&fresh).await;
            let retry = fresh.clone();
            let (source, total) = tokio::task::spawn_blocking(move || open_decoder(&retry))
                .await
                .map_err(|e| e.to_string())??;
            Ok((source, total, fresh))
        }
    }
}

/// Resolves and opens the stream of a track page `url` (used by the player for the current and the next
/// stream song of the queue). Returns the decoder and its duration when known.
pub async fn open_stream(
    app: &AppHandle,
    platform: Platform,
    url: &str,
) -> Result<(BoxedSource, Option<Duration>), String> {
    let started = Instant::now();
    let info = resolve::resolve(app, platform, url).await?;
    let (source, total, info) = open_with_retry(app, platform, url, info).await?;
    println!("[stream] {platform:?} opened in {} ms", started.elapsed().as_millis());
    let duration = total.or(info.duration_ms.map(Duration::from_millis));
    Ok((source, duration))
}

/// Replaces the queue with stream songs (e.g. the clicked search result and the tracks after it) and
/// starts `index`. The player opens the songs in the background (`stream_loading` in the player state)
/// and keeps the next one prepared for a gapless transition.
#[command]
pub fn stream_queue(
    player: State<'_, Mutex<AudioPlayer>>,
    requests: Vec<StreamRequest>,
    index: usize,
) -> Result<(), String> {
    if requests.is_empty() {
        return Err("nothing to play".to_string());
    }
    let songs: Vec<Song> = requests.iter().map(|r| stream_song(r, r.duration_ms)).collect();
    let index = index.min(songs.len() - 1);
    let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
    p.start_playback(songs, index);
    Ok(())
}

/// Warms the url cache (resolve only) so a following stream starts faster. Errors are ignored.
#[command]
pub async fn stream_prefetch(app: AppHandle, platform: Platform, url: String) {
    let Ok(_permit) = prefetch_slots().acquire().await else {
        return;
    };
    let _ = resolve::resolve(&app, platform, &url).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_song_carries_the_request_metadata() {
        let request = StreamRequest {
            platform: Platform::Soundcloud,
            url: "https://soundcloud.com/a/b".into(),
            title: "T".into(),
            artist: "A".into(),
            cover: Some("https://img/x.jpg".into()),
            duration_ms: Some(1234),
        };
        let song = stream_song(&request, Some(5000));
        assert_eq!(song.id, None);
        assert!(song.path.is_empty());
        assert_eq!(song.title, "T");
        assert_eq!(song.artists, vec!["A".to_string()]);
        assert_eq!(song.duration_ms, 5000);
        assert_eq!(song.source, Platform::Soundcloud);
        assert_eq!(song.source_url.as_deref(), Some("https://soundcloud.com/a/b"));
        assert_eq!(song.cover_url.as_deref(), Some("https://img/x.jpg"));
    }

    #[test]
    fn stream_song_empty_cover_is_none_and_unknown_duration_is_zero() {
        let request = StreamRequest {
            platform: Platform::Youtube,
            url: "u".into(),
            title: "T".into(),
            artist: "A".into(),
            cover: Some(String::new()),
            duration_ms: None,
        };
        let song = stream_song(&request, None);
        assert_eq!(song.cover_url, None);
        assert_eq!(song.duration_ms, 0);
    }

    #[test]
    fn request_deserializes_with_optional_fields_missing() {
        let r: StreamRequest = serde_json::from_str(
            r#"{"platform":"youtube","url":"https://y","title":"t","artist":"a"}"#,
        )
        .unwrap();
        assert_eq!(r.platform, Platform::Youtube);
        assert!(r.cover.is_none() && r.duration_ms.is_none());
    }

    #[test]
    fn format_hint_from_content_type_and_url() {
        assert_eq!(format_hint("https://x/a", Some("audio/mpeg")), Some("mp3"));
        assert_eq!(format_hint("https://x/a", Some("audio/mp4")), Some("m4a"));
        assert_eq!(format_hint("https://x/videoplayback?mime=audio%2Fmp4", None), Some("m4a"));
        assert_eq!(format_hint("https://t4.bcbits.com/stream/x/mp3-128/1", None), Some("mp3"));
        assert_eq!(format_hint("https://x/unknown", None), None);
    }

    /// Live check (network): resolve -> HttpSource -> decoder -> first samples -> seek. Run with
    /// `cargo test --lib preview::commands::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_bandcamp_stream_starts_fast_and_seeks() {
        let t0 = Instant::now();
        let info = resolve::resolve_bandcamp("https://mietzeconte.bandcamp.com/track/knows")
            .await
            .expect("resolve");
        let resolved = t0.elapsed();
        let (resolved_ms, open_ms, first_ms, seek_ms, total) = tokio::task::spawn_blocking(move || {
            let t1 = Instant::now();
            let (mut source, total) = open_decoder(&info).expect("open");
            let open = t1.elapsed();
            let first = source.next().expect("first sample");
            assert!(first.is_finite());
            let first_at = t1.elapsed();
            let t2 = Instant::now();
            source.try_seek(Duration::from_secs(60)).expect("seek");
            let after = source.next().expect("sample after seek");
            assert!(after.is_finite());
            (0u128, open.as_millis(), first_at.as_millis(), t2.elapsed().as_millis(), total)
        })
        .await
        .unwrap();
        let _ = resolved_ms;
        println!(
            "resolve {} ms, open {} ms, first sample {} ms, seek to 60s {} ms, duration {:?}",
            resolved.as_millis(), open_ms, first_ms, seek_ms, total
        );
    }

    /// Live check (network + yt-dlp on PATH): the exact yt-dlp arguments of `resolve_youtube`, then the stream.
    #[test]
    #[ignore]
    fn live_youtube_stream_decodes_and_seeks() {
        let t0 = Instant::now();
        let mut args = resolve::youtube_args(resolve::YOUTUBE_FAST_CLIENTS);
        args.push("--".into());
        args.push("https://www.youtube.com/watch?v=FTQbiNvZqaY".into());
        let out = std::process::Command::new("yt-dlp").args(&args).output().expect("yt-dlp");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let info = resolve::parse_youtube_output(&String::from_utf8_lossy(&out.stdout)).expect("parse");
        let resolved = t0.elapsed();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let wait = resolve::wait_before_open(&info, now);
        std::thread::sleep(wait + Duration::from_millis(100));
        println!("waited {} ms for available_at", wait.as_millis());
        let t1 = Instant::now();
        let (mut source, total) = open_decoder(&info).expect("open");
        let first = source.next().expect("first sample");
        assert!(first.is_finite());
        let first_ms = t1.elapsed().as_millis();
        let t2 = Instant::now();
        source.try_seek(Duration::from_secs(90)).expect("seek");
        assert!(source.next().is_some());
        println!(
            "yt-dlp {} ms, open+first sample {} ms, seek {} ms, duration {:?}",
            resolved.as_millis(), first_ms, t2.elapsed().as_millis(), total
        );
    }

    /// Live check (network): SoundCloud track -> (progressive or HLS) stream -> decoder -> first sample -> seek.
    #[tokio::test]
    #[ignore]
    async fn live_soundcloud_stream_starts_and_seeks() {
        let t0 = Instant::now();
        let info = resolve::resolve_soundcloud("https://soundcloud.com/forss/flickermood")
            .await
            .expect("resolve");
        let resolved = t0.elapsed();
        let hls = info.hls.is_some();
        let (open_ms, first_ms, seek_ms, total) = tokio::task::spawn_blocking(move || {
            let t1 = Instant::now();
            let (mut source, total) = open_decoder(&info).expect("open");
            let open = t1.elapsed();
            let first = source.next().expect("first sample");
            assert!(first.is_finite());
            let first_at = t1.elapsed();
            let t2 = Instant::now();
            source.try_seek(Duration::from_secs(60)).expect("seek");
            assert!(source.next().is_some());
            (open.as_millis(), first_at.as_millis(), t2.elapsed().as_millis(), total)
        })
        .await
        .unwrap();
        println!(
            "soundcloud hls={hls}: resolve {} ms, open {open_ms} ms, first sample {first_ms} ms, seek {seek_ms} ms, duration {total:?}",
            resolved.as_millis()
        );
    }
}
