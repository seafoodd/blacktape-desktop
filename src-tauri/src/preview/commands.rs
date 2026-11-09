//! Tauri commands and state of the streaming preview.
//!
//! Flow of `preview_play`: resolve the track page to a direct audio url (cached, see `resolve`) ->
//! open an `HttpSource` (starts downloading, returns with the first bytes) -> build the decoder ->
//! hand the source to the audio player. Every step checks a generation counter so a newer click
//! cancels an older one that is still resolving. State changes are pushed as `preview-state` events.

use crate::audio::player::{AudioPlayer, BoxedSource};
use crate::preview::http_source::HttpSource;
use crate::preview::resolve::{self, StreamInfo};
use crate::types::Platform;
use rodio::{Decoder, Source};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{command, AppHandle, Emitter, Manager, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewStatus {
    Idle,
    Loading,
    Playing,
    Paused,
    Error,
}

/// What the frontend sends to start a preview (metadata is echoed back in the state for the preview bar).
#[derive(Debug, Clone, Deserialize)]
pub struct PreviewRequest {
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

#[derive(Debug, Clone, Serialize)]
pub struct PreviewState {
    pub status: PreviewStatus,
    pub platform: Option<Platform>,
    pub url: Option<String>,
    pub title: String,
    pub artist: String,
    pub cover: Option<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    /// Milliseconds it took from the click to the first sound (set once playing).
    pub startup_ms: Option<u64>,
}

impl PreviewState {
    fn idle() -> Self {
        Self {
            status: PreviewStatus::Idle,
            platform: None,
            url: None,
            title: String::new(),
            artist: String::new(),
            cover: None,
            position_ms: 0,
            duration_ms: None,
            error: None,
            startup_ms: None,
        }
    }
}

struct Inner {
    state: PreviewState,
    generation: u64,
}

pub struct PreviewManager(Mutex<Inner>);

impl Default for PreviewManager {
    fn default() -> Self {
        Self(Mutex::new(Inner {
            state: PreviewState::idle(),
            generation: 0,
        }))
    }
}

impl PreviewManager {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts a new generation and returns its number.
    fn begin(&self, req: &PreviewRequest) -> u64 {
        let mut inner = self.lock();
        inner.generation += 1;
        inner.state = PreviewState {
            status: PreviewStatus::Loading,
            platform: Some(req.platform),
            url: Some(req.url.clone()),
            title: req.title.clone(),
            artist: req.artist.clone(),
            cover: req.cover.clone(),
            position_ms: 0,
            duration_ms: req.duration_ms,
            error: None,
            startup_ms: None,
        };
        inner.generation
    }

    fn is_current(&self, generation: u64) -> bool {
        self.lock().generation == generation
    }

    /// Applies `f` to the state when `generation` is still current; returns the new state to emit.
    fn update(&self, generation: u64, f: impl FnOnce(&mut PreviewState)) -> Option<PreviewState> {
        let mut inner = self.lock();
        if inner.generation != generation {
            return None;
        }
        f(&mut inner.state);
        Some(inner.state.clone())
    }

    fn snapshot(&self) -> PreviewState {
        self.lock().state.clone()
    }

    /// Cancels everything in flight and goes idle.
    fn reset(&self) -> PreviewState {
        let mut inner = self.lock();
        inner.generation += 1;
        inner.state = PreviewState::idle();
        inner.state.clone()
    }
}

fn emit(app: &AppHandle, state: &PreviewState) {
    let _ = app.emit("preview-state", state);
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
fn open_decoder(
    url: &str,
    headers: &[(String, String)],
) -> Result<(BoxedSource, Option<Duration>), String> {
    let client = if headers.is_empty() {
        blocking_client().clone()
    } else {
        client_with_headers(headers)?
    };
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

async fn open_with_retry(
    app: &AppHandle,
    req: &PreviewRequest,
    info: StreamInfo,
) -> Result<(BoxedSource, Option<Duration>, StreamInfo), String> {
    let first_url = info.url.clone();
    let first_headers = info.headers.clone();
    match tokio::task::spawn_blocking(move || open_decoder(&first_url, &first_headers))
        .await
        .map_err(|e| e.to_string())?
    {
        Ok((source, total)) => Ok((source, total, info)),
        Err(first_error) => {
            // The cached url may have expired: look the track up again once.
            eprintln!("[preview] open failed ({first_error}), re-resolving");
            resolve::forget(req.platform, &req.url).await;
            let fresh = resolve::resolve(app, req.platform, &req.url).await?;
            let retry_url = fresh.url.clone();
            let retry_headers = fresh.headers.clone();
            let (source, total) = tokio::task::spawn_blocking(move || open_decoder(&retry_url, &retry_headers))
                .await
                .map_err(|e| e.to_string())??;
            Ok((source, total, fresh))
        }
    }
}

#[command]
pub async fn preview_play(
    app: AppHandle,
    manager: State<'_, PreviewManager>,
    player: State<'_, Mutex<AudioPlayer>>,
    request: PreviewRequest,
) -> Result<(), String> {
    let started = Instant::now();
    let generation = manager.begin(&request);
    emit(&app, &manager.snapshot());
    // Silence whatever preview is currently audible while the new one loads.
    if let Ok(mut p) = player.lock() {
        p.preview_stop();
    }

    let result = async {
        let info = resolve::resolve(&app, request.platform, &request.url).await?;
        if !manager.is_current(generation) {
            return Ok(false);
        }
        let (source, total, info) = open_with_retry(&app, &request, info).await?;
        if !manager.is_current(generation) {
            return Ok(false);
        }
        let duration_ms = total
            .map(|d| d.as_millis() as u64)
            .or(info.duration_ms)
            .or(request.duration_ms);
        {
            let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
            // Re-check under the lock: a newer preview must not be overridden.
            if !manager.is_current(generation) {
                return Ok(false);
            }
            p.preview_start(source);
        }
        if let Some(state) = manager.update(generation, |s| {
            s.status = PreviewStatus::Playing;
            s.duration_ms = duration_ms;
            s.startup_ms = Some(started.elapsed().as_millis() as u64);
        }) {
            println!(
                "[preview] {:?} started in {} ms",
                request.platform,
                started.elapsed().as_millis()
            );
            emit(&app, &state);
        }
        Ok::<bool, String>(true)
    }
    .await;

    match result {
        Ok(_) => Ok(()),
        Err(message) => {
            if let Some(state) = manager.update(generation, |s| {
                s.status = PreviewStatus::Error;
                s.error = Some(message.clone());
            }) {
                emit(&app, &state);
            }
            Err(message)
        }
    }
}

/// Warms the url cache (resolve only) so a following `preview_play` starts faster. Errors are ignored.
#[command]
pub async fn preview_prefetch(app: AppHandle, platform: Platform, url: String) {
    let _ = resolve::resolve(&app, platform, &url).await;
}

#[command]
pub fn preview_toggle(
    app: AppHandle,
    manager: State<'_, PreviewManager>,
    player: State<'_, Mutex<AudioPlayer>>,
) {
    let Ok(mut p) = player.lock() else { return };
    if !p.preview_is_active() {
        return;
    }
    p.preview_toggle();
    let paused = p.preview_is_paused();
    drop(p);
    let generation = manager.lock().generation;
    if let Some(state) = manager.update(generation, |s| {
        s.status = if paused {
            PreviewStatus::Paused
        } else {
            PreviewStatus::Playing
        }
    }) {
        emit(&app, &state);
    }
}

/// Seeks to `fraction` (0..1) of the preview.
#[command]
pub fn preview_seek(
    app: AppHandle,
    manager: State<'_, PreviewManager>,
    player: State<'_, Mutex<AudioPlayer>>,
    fraction: f32,
) -> Result<(), String> {
    let snapshot = manager.snapshot();
    let duration_ms = snapshot.duration_ms.ok_or("duration unknown")?;
    let target = (duration_ms as f64 * f64::from(fraction.clamp(0.0, 1.0))) as u64;
    {
        let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
        p.preview_seek(Duration::from_millis(target))?;
    }
    let generation = manager.lock().generation;
    if let Some(state) = manager.update(generation, |s| s.position_ms = target) {
        emit(&app, &state);
    }
    Ok(())
}

#[command]
pub fn preview_stop(
    app: AppHandle,
    manager: State<'_, PreviewManager>,
    player: State<'_, Mutex<AudioPlayer>>,
) {
    if let Ok(mut p) = player.lock() {
        p.preview_stop();
    }
    emit(&app, &manager.reset());
}

#[command]
pub fn preview_get_state(manager: State<'_, PreviewManager>) -> PreviewState {
    manager.snapshot()
}

/// Background loop: pushes the playback position while a preview plays and ends it when the audio ran out.
pub fn spawn_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let manager = app.state::<PreviewManager>();
            let snapshot = manager.snapshot();
            if !matches!(snapshot.status, PreviewStatus::Playing | PreviewStatus::Paused) {
                continue;
            }
            let player = app.state::<Mutex<AudioPlayer>>();
            let (active, finished, position) = match player.lock() {
                Ok(p) => (p.preview_is_active(), p.preview_finished(), p.preview_position()),
                Err(_) => continue,
            };
            let generation = manager.lock().generation;
            if !active || finished {
                if let Ok(mut p) = player.lock() {
                    p.preview_stop();
                }
                emit(&app, &manager.reset());
                continue;
            }
            if snapshot.status == PreviewStatus::Playing {
                if let Some(state) = manager.update(generation, |s| {
                    s.position_ms = position.as_millis() as u64;
                }) {
                    emit(&app, &state);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(url: &str) -> PreviewRequest {
        PreviewRequest {
            platform: Platform::Bandcamp,
            url: url.to_string(),
            title: "t".into(),
            artist: "a".into(),
            cover: None,
            duration_ms: Some(1000),
        }
    }

    #[test]
    fn begin_goes_loading_and_newer_generation_wins() {
        let manager = PreviewManager::default();
        let first = manager.begin(&request("u1"));
        assert_eq!(manager.snapshot().status, PreviewStatus::Loading);
        let second = manager.begin(&request("u2"));
        assert!(second > first);
        assert!(!manager.is_current(first));
        assert!(manager.is_current(second));
        assert_eq!(manager.snapshot().url.as_deref(), Some("u2"));
    }

    #[test]
    fn stale_generation_cannot_update_state() {
        let manager = PreviewManager::default();
        let first = manager.begin(&request("u1"));
        let second = manager.begin(&request("u2"));
        assert!(manager.update(first, |s| s.status = PreviewStatus::Playing).is_none());
        assert_eq!(manager.snapshot().status, PreviewStatus::Loading);
        assert!(manager.update(second, |s| s.status = PreviewStatus::Playing).is_some());
        assert_eq!(manager.snapshot().status, PreviewStatus::Playing);
    }

    #[test]
    fn reset_invalidates_in_flight_work_and_goes_idle() {
        let manager = PreviewManager::default();
        let generation = manager.begin(&request("u1"));
        let state = manager.reset();
        assert_eq!(state.status, PreviewStatus::Idle);
        assert!(!manager.is_current(generation));
        assert!(state.url.is_none());
    }

    #[test]
    fn state_serializes_lowercase_status() {
        let json = serde_json::to_value(PreviewState::idle()).unwrap();
        assert_eq!(json["status"], "idle");
        assert!(json.get("position_ms").is_some());
    }

    #[test]
    fn request_deserializes_with_optional_fields_missing() {
        let r: PreviewRequest = serde_json::from_str(
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
        let url = info.url.clone();
        let (resolved_ms, open_ms, first_ms, seek_ms, total) = tokio::task::spawn_blocking(move || {
            let t1 = Instant::now();
            let (mut source, total) = open_decoder(&url, &[]).expect("open");
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
        let args = resolve::youtube_args("https://www.youtube.com/watch?v=FTQbiNvZqaY");
        let out = std::process::Command::new("yt-dlp").args(&args).output().expect("yt-dlp");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let info = resolve::parse_youtube_output(&String::from_utf8_lossy(&out.stdout)).expect("parse");
        let resolved = t0.elapsed();
        let t1 = Instant::now();
        let (mut source, total) = open_decoder(&info.url, &info.headers).expect("open");
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
}
