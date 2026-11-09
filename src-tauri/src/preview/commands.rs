//! Streaming playback: `open_stream` resolves a track page to a direct audio url, opens it (HTTP or HLS) and
//! plays it through the normal audio player (`AudioPlayer::play_stream`), so the existing player controls,
//! queue handling, media keys and volume all apply. `stream_prefetch` warms the url cache.

use crate::audio::player::{AudioPlayer, BoxedSource};
use crate::preview::http_source::HttpSource;
use crate::preview::resolve::{self, StreamInfo};
use crate::preview::segmented_source::SegmentedSource;
use crate::types::{Platform, QualityTier, Song};
use rand::RngExt;
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
    /// Album the track belongs to, shown by the player.
    #[serde(default)]
    pub album: Option<String>,
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
        album: request.album.clone().unwrap_or_default(),
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

/// Which song `stream_queue` starts: the requested `index` (clamped to the list), or a random one when
/// `shuffle` is on. `roll(n)` returns a number in `0..n` (injected so the choice is testable). `len` > 0.
fn start_index(len: usize, index: usize, shuffle: bool, roll: impl FnOnce(usize) -> usize) -> usize {
    if shuffle {
        roll(len).min(len - 1)
    } else {
        index.min(len - 1)
    }
}

/// How `stream_add_to_queue` puts new songs into the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueAdd {
    /// Nothing is queued: play the songs from the first one.
    StartPlayback,
    /// Right after the current song, keeping the given order.
    PlayNext,
    /// At the end of the queue, keeping the given order.
    Append,
}

fn queue_add_mode(queue_len: usize, play_next: bool) -> QueueAdd {
    if queue_len == 0 {
        QueueAdd::StartPlayback
    } else if play_next {
        QueueAdd::PlayNext
    } else {
        QueueAdd::Append
    }
}

/// Replaces the queue with stream songs (e.g. the clicked search result and the tracks after it) and
/// starts `index`. The player opens the songs in the background (`stream_loading` in the player state)
/// and keeps the next one prepared for a gapless transition.
///
/// `shuffle = Some(true)` (the "Shuffle" button): starts at a random song and turns shuffle mode on (part B;
/// `index` is ignored then). `None`/`Some(false)`: unchanged behaviour.
#[command]
pub fn stream_queue(
    player: State<'_, Mutex<AudioPlayer>>,
    requests: Vec<StreamRequest>,
    index: usize,
    shuffle: Option<bool>,
) -> Result<(), String> {
    if requests.is_empty() {
        return Err("nothing to play".to_string());
    }
    let shuffle = shuffle == Some(true);
    let songs: Vec<Song> = requests.iter().map(|r| stream_song(r, r.duration_ms)).collect();
    let index = start_index(songs.len(), index, shuffle, |n| rand::rng().random_range(0..n));
    let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
    if shuffle && !p.is_shuffle() {
        // Turned on BEFORE the playback starts: `start_playback` then shuffles the new queue with the chosen
        // song first, and the next stream to prepare is picked from the final order.
        p.toggle_shuffle();
    }
    p.start_playback(songs, index);
    Ok(())
}

/// One entry of a mixed queue: a library song (by id) or a song to stream.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MixedEntry {
    Song { id: i64 },
    Stream(StreamRequest),
}

/// Index in the resolved queue of the entry at `index` of the requested one. `resolved[i]` tells whether entry
/// `i` could be built (a library song that is gone is dropped). When the requested entry was dropped, the next
/// resolved one is used (or the last).
pub fn mixed_start_index(resolved: &[bool], index: usize) -> usize {
    let before = resolved.iter().take(index).filter(|r| **r).count();
    let total = resolved.iter().filter(|r| **r).count();
    before.min(total.saturating_sub(1))
}

/// Replaces the queue with library songs and streams in the given order and starts `index`. The whole list is
/// the queue, so the entries before `index` stay reachable with "previous".
#[command]
pub async fn start_mixed_playback(
    entries: Vec<MixedEntry>,
    index: usize,
    db_state: State<'_, tokio::sync::Mutex<crate::db::db::Database>>,
    player: State<'_, Mutex<AudioPlayer>>,
) -> Result<(), String> {
    let ids: Vec<i64> = entries
        .iter()
        .filter_map(|e| if let MixedEntry::Song { id } = e { Some(*id) } else { None })
        .collect();
    let mut by_id: std::collections::HashMap<i64, Song> = if ids.is_empty() {
        std::collections::HashMap::new()
    } else {
        let db = db_state.lock().await;
        db.get_songs_by_ids(&ids)
            .await
            .map_err(|e| format!("SQLx failed to fetch songs: {e:?}"))?
            .into_iter()
            .filter_map(|s| s.id.map(|id| (id, s)))
            .collect()
    };
    let built: Vec<Option<Song>> = entries
        .iter()
        .map(|e| match e {
            MixedEntry::Song { id } => by_id.get(id).cloned(),
            MixedEntry::Stream(r) => Some(stream_song(r, r.duration_ms)),
        })
        .collect();
    by_id.clear();
    let resolved: Vec<bool> = built.iter().map(Option::is_some).collect();
    let songs: Vec<Song> = built.into_iter().flatten().collect();
    if songs.is_empty() {
        return Err("nothing to play".to_string());
    }
    let start = mixed_start_index(&resolved, index);
    let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
    p.start_playback(songs, start);
    Ok(())
}

/// Adds stream songs to the queue WITHOUT replacing it: right after the current song (in the given order)
/// when `play_next` is true, at the end otherwise. When nothing is queued it starts playing them (part B).
#[command]
pub fn stream_add_to_queue(
    player: State<'_, Mutex<AudioPlayer>>,
    requests: Vec<StreamRequest>,
    play_next: Option<bool>,
) -> Result<(), String> {
    if requests.is_empty() {
        return Ok(());
    }
    let songs: Vec<Song> = requests.iter().map(|r| stream_song(r, r.duration_ms)).collect();
    let mut p = player.lock().map_err(|_| "player lock poisoned".to_string())?;
    match queue_add_mode(p.queue_len(), play_next == Some(true)) {
        QueueAdd::StartPlayback => p.start_playback(songs, 0),
        QueueAdd::PlayNext => p.add_songs_to_queue(songs, true),
        QueueAdd::Append => p.add_songs_to_queue(songs, false),
    }
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
            album: Some("Alb".into()),
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
        assert_eq!(song.album, "Alb");
    }

    #[test]
    fn stream_song_empty_cover_is_none_and_unknown_duration_is_zero() {
        let request = StreamRequest {
            platform: Platform::Youtube,
            url: "u".into(),
            title: "T".into(),
            artist: "A".into(),
            album: None,
            cover: Some(String::new()),
            duration_ms: None,
        };
        let song = stream_song(&request, None);
        assert_eq!(song.cover_url, None);
        assert_eq!(song.duration_ms, 0);
        assert_eq!(song.album, "");
    }

    #[test]
    fn request_deserializes_with_optional_fields_missing() {
        let r: StreamRequest = serde_json::from_str(
            r#"{"platform":"youtube","url":"https://y","title":"t","artist":"a"}"#,
        )
        .unwrap();
        assert_eq!(r.platform, Platform::Youtube);
        assert!(r.cover.is_none() && r.duration_ms.is_none() && r.album.is_none());
    }

    #[test]
    fn start_index_without_shuffle_uses_the_requested_index_clamped() {
        let never = |_: usize| -> usize { panic!("no roll without shuffle") };
        assert_eq!(start_index(5, 2, false, never), 2);
        assert_eq!(start_index(5, 99, false, never), 4);
        assert_eq!(start_index(1, 0, false, never), 0);
    }

    #[test]
    fn start_index_with_shuffle_ignores_the_index_and_uses_the_roll() {
        assert_eq!(start_index(5, 2, true, |n| { assert_eq!(n, 5); 3 }), 3);
        assert_eq!(start_index(5, 4, true, |_| 0), 0);
    }

    #[test]
    fn start_index_with_shuffle_never_leaves_the_list() {
        assert_eq!(start_index(3, 0, true, |_| 10), 2);
        assert_eq!(start_index(1, 0, true, |n| n), 0);
    }

    #[test]
    fn mixed_start_index_keeps_the_index_when_everything_resolved() {
        assert_eq!(mixed_start_index(&[true; 4], 0), 0);
        assert_eq!(mixed_start_index(&[true; 4], 2), 2);
        assert_eq!(mixed_start_index(&[true; 4], 3), 3);
    }

    #[test]
    fn mixed_start_index_shifts_down_for_unresolved_entries_before_it() {
        assert_eq!(mixed_start_index(&[false, true, true, true], 2), 1);
        assert_eq!(mixed_start_index(&[true, false, true], 2), 1);
        assert_eq!(mixed_start_index(&[false, false, true], 2), 0);
    }

    #[test]
    fn mixed_start_index_uses_the_next_resolved_entry_when_the_requested_one_is_gone() {
        assert_eq!(mixed_start_index(&[true, false, true], 1), 1);
        assert_eq!(mixed_start_index(&[true, true, false, true], 2), 2);
    }

    #[test]
    fn mixed_start_index_clamps_to_the_last_resolved_when_past_the_end() {
        assert_eq!(mixed_start_index(&[true, true, true], 99), 2);
        assert_eq!(mixed_start_index(&[true, false, false], 5), 0);
    }

    #[test]
    fn mixed_start_index_is_zero_when_nothing_resolved_or_empty() {
        assert_eq!(mixed_start_index(&[false, false], 1), 0);
        assert_eq!(mixed_start_index(&[], 0), 0);
        assert_eq!(mixed_start_index(&[], 5), 0);
    }

    #[test]
    fn mixed_entry_deserializes_a_library_song() {
        let e: MixedEntry = serde_json::from_str(r#"{"kind":"song","id":7}"#).unwrap();
        match e {
            MixedEntry::Song { id } => assert_eq!(id, 7),
            other => panic!("expected Song, got {other:?}"),
        }
    }

    #[test]
    fn mixed_entry_deserializes_a_stream() {
        let e: MixedEntry = serde_json::from_str(
            r#"{"kind":"stream","platform":"youtube","url":"u","title":"t","artist":"a"}"#,
        )
        .unwrap();
        match e {
            MixedEntry::Stream(r) => {
                assert_eq!(r.platform, Platform::Youtube);
                assert_eq!(r.url, "u");
                assert_eq!(r.title, "t");
                assert_eq!(r.artist, "a");
            }
            other => panic!("expected Stream, got {other:?}"),
        }
    }

    #[test]
    fn real_roll_stays_in_range() {
        for _ in 0..200 {
            let i = start_index(4, 0, true, |n| rand::rng().random_range(0..n));
            assert!(i < 4);
        }
    }

    #[test]
    fn queue_add_mode_starts_playback_only_on_an_empty_queue() {
        assert_eq!(queue_add_mode(0, false), QueueAdd::StartPlayback);
        assert_eq!(queue_add_mode(0, true), QueueAdd::StartPlayback);
    }

    #[test]
    fn queue_add_mode_never_replaces_a_filled_queue() {
        assert_eq!(queue_add_mode(3, true), QueueAdd::PlayNext);
        assert_eq!(queue_add_mode(3, false), QueueAdd::Append);
        assert_eq!(queue_add_mode(1, false), QueueAdd::Append);
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
