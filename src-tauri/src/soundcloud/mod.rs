//! Minimal client for SoundCloud's public web API (`api-v2.soundcloud.com`).
//!
//! This is the same API the soundcloud.com web player uses. It needs a `client_id`, which
//! is public and is read from the web app (the page's hydration block, or one of its
//! `a-v2.sndcdn.com/assets/*.js` bundles). The id is cached for the life of the process and
//! fetched again once when the API rejects it (401/403).
//!
//! Search lives in `crate::search::soundcloud` and download metadata in
//! `crate::download::soundcloud`. The audio itself is downloaded by yt-dlp, like every
//! other platform.

use regex::Regex;
use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

pub const API_BASE: &str = "https://api-v2.soundcloud.com";
const HOME_URL: &str = "https://soundcloud.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
/// `/tracks?ids=` accepts at most 50 ids per request.
const TRACK_IDS_CHUNK: usize = 50;

static CLIENT_ID: RwLock<Option<String>> = RwLock::new(None);

// ---------------------------------------------------------------------------------------
// API models (only the fields we use; everything is optional because api-v2 omits fields
// freely, and set track lists contain "stub" tracks that carry only an id).
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ScUser {
    pub id: u64,
    #[serde(default)]
    pub username: String,
    pub full_name: Option<String>,
    pub permalink_url: Option<String>,
    pub avatar_url: Option<String>,
    pub followers_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScFormat {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub mime_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScTranscoding {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub snipped: bool,
    pub duration: Option<u64>,
    pub quality: Option<String>,
    pub format: ScFormat,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScMedia {
    #[serde(default)]
    pub transcodings: Vec<ScTranscoding>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScPublisherMetadata {
    pub artist: Option<String>,
    pub album_title: Option<String>,
    pub release_title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScTrack {
    pub id: u64,
    /// `None` for stub tracks inside a set, which carry only `id`/`policy`.
    pub title: Option<String>,
    pub permalink_url: Option<String>,
    pub secret_token: Option<String>,
    pub artwork_url: Option<String>,
    /// Playable length in ms (30 000 for Go+ previews).
    pub duration: Option<u64>,
    /// Real length of the track in ms.
    pub full_duration: Option<u64>,
    /// `ALLOW`, `MONETIZE`, `SNIP` (Go+ preview only) or `BLOCK` (geo-blocked).
    pub policy: Option<String>,
    pub monetization_model: Option<String>,
    pub streamable: Option<bool>,
    pub genre: Option<String>,
    pub release_date: Option<String>,
    pub display_date: Option<String>,
    pub playback_count: Option<u64>,
    pub publisher_metadata: Option<ScPublisherMetadata>,
    pub media: Option<ScMedia>,
    pub user: Option<ScUser>,
}

impl ScTrack {
    /// Stub tracks in a set only carry an id; the full object has to be fetched.
    pub fn is_stub(&self) -> bool {
        self.title.is_none() || self.media.is_none()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScPlaylist {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    pub permalink_url: Option<String>,
    pub artwork_url: Option<String>,
    pub genre: Option<String>,
    pub release_date: Option<String>,
    pub published_at: Option<String>,
    pub display_date: Option<String>,
    pub is_album: Option<bool>,
    pub set_type: Option<String>,
    pub track_count: Option<u32>,
    pub user: Option<ScUser>,
    #[serde(default)]
    pub tracks: Vec<ScTrack>,
}

/// What `/resolve` returns, discriminated by the `kind` field.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum ScResource {
    #[serde(rename = "track")]
    Track(ScTrack),
    #[serde(rename = "playlist")]
    Playlist(ScPlaylist),
    #[serde(rename = "user")]
    User(ScUser),
    #[serde(other)]
    Other,
}

/// A page of search results. Items are kept as raw JSON so one malformed item doesn't
/// fail the whole page.
#[derive(Debug, Clone, Deserialize)]
pub struct ScCollection {
    #[serde(default)]
    pub collection: Vec<serde_json::Value>,
    pub next_href: Option<String>,
    pub total_results: Option<u64>,
}

impl ScCollection {
    pub fn items<T: DeserializeOwned>(&self) -> Vec<T> {
        self.collection
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect()
    }
}

// ---------------------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------------------

/// SoundCloud serves artwork as `...-large.jpg` (100x100). Ask for the 500x500 rendition.
/// `-original` and `-t500x500` are left alone.
pub fn upgrade_artwork_url(url: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"-(?:large|crop|small|badge|tiny|mini|t\d+x\d+)\.(jpg|jpeg|png|webp)").unwrap()
    });
    re.replace(url, "-t500x500.$1").into_owned()
}

/// Best cover for a track: its artwork, else the uploader's avatar.
pub fn track_artwork(track: &ScTrack) -> Option<String> {
    track
        .artwork_url
        .as_deref()
        .or_else(|| track.user.as_ref().and_then(|u| u.avatar_url.as_deref()))
        .filter(|u| !u.is_empty())
        .map(upgrade_artwork_url)
}

/// Best cover for a set: its artwork, else the first track's artwork, else the avatar.
pub fn playlist_artwork(playlist: &ScPlaylist) -> Option<String> {
    playlist
        .artwork_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .map(upgrade_artwork_url)
        .or_else(|| {
            playlist
                .tracks
                .iter()
                .find_map(|t| t.artwork_url.as_deref().filter(|u| !u.is_empty()))
                .map(upgrade_artwork_url)
        })
        .or_else(|| {
            playlist
                .user
                .as_ref()
                .and_then(|u| u.avatar_url.as_deref())
                .filter(|u| !u.is_empty())
                .map(upgrade_artwork_url)
        })
}

/// Year of the first date that parses (`"2003-06-02T00:00:00Z"` -> 2003).
pub fn year_from_dates(dates: &[Option<&str>]) -> Option<i32> {
    dates.iter().flatten().find_map(|d| {
        let year: i32 = d.get(0..4)?.parse().ok()?;
        (1000..=9999).contains(&year).then_some(year)
    })
}

/// `offset` query parameter of a `next_href`, used for paging search results.
pub fn next_offset_from_href(next_href: Option<&str>) -> Option<u32> {
    let parsed = url::Url::parse(next_href?).ok()?;
    let offset = parsed
        .query_pairs()
        .find(|(k, _)| k == "offset")
        .and_then(|(_, v)| v.parse().ok());
    offset
}

pub fn is_soundcloud_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .map(|h| h == "soundcloud.com" || h.ends_with(".soundcloud.com") || h == "snd.sc")
        .unwrap_or(false)
}

/// Picks the stream a full download should use, or `None` when only previews exist.
///
/// Snipped (30 s Go+ preview) and encrypted (DRM) streams are never chosen. Progressive
/// MP3 is preferred, then plain HLS by bitrate. SoundCloud has moved most tracks to
/// HLS-only (AAC 160k / 96k, MP3, Opus), so HLS is the common case.
pub fn choose_transcoding(transcodings: &[ScTranscoding]) -> Option<&ScTranscoding> {
    transcodings
        .iter()
        .filter(|t| !t.snipped && !t.url.contains("/preview/") && !t.url.is_empty())
        .filter_map(|t| transcoding_score(t).map(|s| (s, t)))
        .max_by_key(|(s, _)| *s)
        .map(|(_, t)| t)
}

fn transcoding_score(t: &ScTranscoding) -> Option<u32> {
    let protocol_rank = match t.format.protocol.as_str() {
        "progressive" => 3,
        "hls" => 2,
        // "ctr-encrypted-hls", "cbc-encrypted-hls" and anything new: not usable.
        _ => return None,
    };
    let mime = t.format.mime_type.as_str();
    let is_mp3 = mime.contains("audio/mpeg") || t.preset.starts_with("mp3");
    // Adaptive master playlists ("abr_*", audio/mpegurl) are a fallback only.
    let is_adaptive = t.preset.starts_with("abr") || mime.contains("mpegurl");
    let bitrate = preset_bitrate(&t.preset);
    let quality_rank = match t.quality.as_deref() {
        Some("hq") => 3,
        Some("sq") => 2,
        Some("lq") => 1,
        _ => 0,
    };
    let mp3_bonus = u32::from(protocol_rank == 3 && is_mp3);
    let adaptive_penalty = u32::from(!is_adaptive);
    Some(
        protocol_rank * 100_000
            + mp3_bonus * 50_000
            + adaptive_penalty * 10_000
            + quality_rank * 1_000
            + bitrate,
    )
}

/// `"aac_160k"` -> 160, `"mp3_1_0"`/`"mp3_0_0"` -> 128 (SoundCloud's MP3 is 128k),
/// `"opus_0_0"` -> 64, anything else -> 0.
fn preset_bitrate(preset: &str) -> u32 {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"_(\d+)k").unwrap());
    if let Some(caps) = re.captures(preset) {
        return caps[1].parse().unwrap_or(0);
    }
    if preset.starts_with("mp3") {
        128
    } else if preset.starts_with("opus") {
        64
    } else {
        0
    }
}

/// Whether a track can be downloaded in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Full,
    /// Only a 30 s preview is available (Go+ / "SNIP" policy).
    PreviewOnly,
    /// Not available in the current country.
    GeoBlocked,
    /// Not streamable, no usable stream, or no metadata.
    Unavailable(String),
}

impl Availability {
    pub fn reason(&self) -> Option<String> {
        match self {
            Availability::Full => None,
            Availability::PreviewOnly => Some(
                "only a 30-second preview is available (SoundCloud Go+ track); \
                 the full track can't be downloaded"
                    .into(),
            ),
            Availability::GeoBlocked => Some("not available in your country".into()),
            Availability::Unavailable(why) => Some(why.clone()),
        }
    }
}

pub fn availability(track: &ScTrack) -> Availability {
    if track.is_stub() {
        return Availability::Unavailable("track metadata could not be loaded".into());
    }
    match track.policy.as_deref() {
        Some("BLOCK") => return Availability::GeoBlocked,
        Some("SNIP") => return Availability::PreviewOnly,
        _ => {}
    }
    if track.streamable == Some(false) {
        return Availability::Unavailable("the uploader disabled streaming".into());
    }
    let transcodings = track
        .media
        .as_ref()
        .map(|m| m.transcodings.as_slice())
        .unwrap_or_default();
    if transcodings.is_empty() {
        return Availability::Unavailable("no audio streams are published".into());
    }
    if choose_transcoding(transcodings).is_none() {
        let any_preview = transcodings
            .iter()
            .any(|t| t.snipped || t.url.contains("/preview/"));
        return if any_preview {
            Availability::PreviewOnly
        } else {
            Availability::Unavailable("only DRM-protected streams are available".into())
        };
    }
    // A 30 s "duration" on a much longer track is a preview even if the flags say otherwise.
    if let (Some(d), Some(full)) = (track.duration, track.full_duration) {
        if d <= 30_000 && full > d + 1_000 {
            return Availability::PreviewOnly;
        }
    }
    Availability::Full
}

/// Splits SoundCloud's usual "Artist - Title" upload naming.
///
/// The artist is `publisher_metadata.artist` when present, else the uploader's username.
/// The title loses an "Artist - " prefix only when that prefix matches the chosen artist,
/// so titles like "Song - Remix" stay intact.
pub fn artist_and_title(track: &ScTrack) -> (String, String) {
    let raw_title = track.title.clone().unwrap_or_default().trim().to_string();
    let publisher_artist = track
        .publisher_metadata
        .as_ref()
        .and_then(|p| p.artist.as_deref())
        .map(str::trim)
        .filter(|a| !a.is_empty());
    let username = track
        .user
        .as_ref()
        .map(|u| u.username.trim())
        .filter(|u| !u.is_empty());

    let artist = publisher_artist
        .or(username)
        .unwrap_or("Unknown Artist")
        .to_string();

    let title = strip_artist_prefix(&raw_title, &artist)
        .or_else(|| username.and_then(|u| strip_artist_prefix(&raw_title, u)))
        .unwrap_or(raw_title.clone());

    let title = if title.is_empty() { raw_title } else { title };
    (artist, title)
}

fn strip_artist_prefix(title: &str, artist: &str) -> Option<String> {
    for sep in [" - ", " – ", " — "] {
        let prefix = format!("{artist}{sep}");
        if title.len() > prefix.len()
            && title
                .get(..prefix.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(&prefix))
        {
            return Some(title[prefix.len()..].trim().to_string());
        }
    }
    None
}

/// URL yt-dlp should download. Private ("secret link") tracks need the token appended.
pub fn track_download_url(track: &ScTrack) -> Option<String> {
    let base = track.permalink_url.as_deref()?.trim_end_matches('/');
    Some(match track.secret_token.as_deref().filter(|s| !s.is_empty()) {
        Some(token) if !base.ends_with(token) => format!("{base}/{token}"),
        _ => base.to_string(),
    })
}

/// `client_id` from the page's `__sc_hydration` block (`{"hydratable":"apiClient",...}`).
pub fn extract_client_id_from_hydration(html: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([A-Za-z0-9_-]{16,64})""#,
        )
        .unwrap()
    });
    re.captures(html).map(|c| c[1].to_string())
}

/// App bundle URLs referenced by the soundcloud.com page, in page order.
pub fn extract_bundle_urls(html: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"<script[^>]*\ssrc="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#).unwrap()
    });
    let mut urls: Vec<String> = Vec::new();
    for caps in re.captures_iter(html) {
        let u = caps[1].to_string();
        if !urls.contains(&u) {
            urls.push(u);
        }
    }
    urls
}

/// `client_id` from an app bundle (`client_id:"…"` or `?client_id=…`).
pub fn extract_client_id_from_bundle(js: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"client_id\s*[:=]\s*"([A-Za-z0-9_-]{16,64})"|[?&]client_id=([A-Za-z0-9_-]{16,64})"#)
            .unwrap()
    });
    re.captures(js)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_string())
}

// ---------------------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------------------

fn http() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let resp = http()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("SoundCloud request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("SoundCloud returned {} for {url}", resp.status()));
    }
    resp.text()
        .await
        .map_err(|e| format!("SoundCloud response could not be read: {e}"))
}

/// Reads a fresh `client_id` from soundcloud.com: hydration block first, then the app
/// bundles (the id usually sits in one of the last ones, so they're scanned from the end).
async fn fetch_client_id() -> Result<String, String> {
    let html = fetch_text(HOME_URL).await?;
    if let Some(id) = extract_client_id_from_hydration(&html) {
        return Ok(id);
    }
    for bundle in extract_bundle_urls(&html).iter().rev() {
        if let Ok(js) = fetch_text(bundle).await {
            if let Some(id) = extract_client_id_from_bundle(&js) {
                return Ok(id);
            }
        }
    }
    Err("Could not obtain a SoundCloud client id (the web app layout may have changed).".into())
}

async fn client_id(force_refresh: bool) -> Result<String, String> {
    if !force_refresh {
        if let Some(id) = CLIENT_ID.read().ok().and_then(|g| g.clone()) {
            return Ok(id);
        }
    }
    let id = fetch_client_id().await?;
    if let Ok(mut guard) = CLIENT_ID.write() {
        *guard = Some(id.clone());
    }
    Ok(id)
}

/// GET an api-v2 endpoint (`path` may be relative to [`API_BASE`] or a full api-v2 URL,
/// e.g. a `next_href`). Retries once with a fresh client id on 401/403.
pub async fn api_get<T: DeserializeOwned>(path: &str, params: &[(&str, String)]) -> Result<T, String> {
    let base = if path.starts_with("http") {
        path.to_string()
    } else {
        format!("{API_BASE}{path}")
    };

    let mut refreshed = false;
    loop {
        let id = client_id(refreshed).await?;
        let url = build_api_url(&base, params, &id)?;
        let resp = http()
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("SoundCloud request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            let body = resp
                .text()
                .await
                .map_err(|e| format!("SoundCloud response could not be read: {e}"))?;
            return serde_json::from_str(&body)
                .map_err(|e| format!("Unexpected SoundCloud response ({e})"));
        }
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) && !refreshed {
            refreshed = true;
            continue;
        }
        return Err(describe_status(status));
    }
}

/// Adds `params` and `client_id` to `base`. Parameters already in `base` (a `next_href`
/// carries `q`, `offset`, …) are replaced, and any stale `client_id` is dropped.
pub fn build_api_url(base: &str, params: &[(&str, String)], client_id: &str) -> Result<url::Url, String> {
    let mut url = url::Url::parse(base).map_err(|e| format!("Bad SoundCloud URL {base}: {e}"))?;
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "client_id" && !params.iter().any(|(p, _)| p == k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    {
        let mut q = url.query_pairs_mut();
        q.clear();
        for (k, v) in &kept {
            q.append_pair(k, v);
        }
        for (k, v) in params {
            q.append_pair(k, v);
        }
        q.append_pair("client_id", client_id);
    }
    Ok(url)
}

fn describe_status(status: StatusCode) -> String {
    match status {
        StatusCode::NOT_FOUND => "SoundCloud: not found (it may be private or deleted).".into(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            "SoundCloud refused the request (private, region-locked, or the client id was rejected)."
                .into()
        }
        StatusCode::TOO_MANY_REQUESTS => {
            "SoundCloud rate limit reached. Wait a minute and try again.".into()
        }
        s => format!("SoundCloud API error: {s}"),
    }
}

pub async fn resolve(url: &str) -> Result<ScResource, String> {
    api_get("/resolve", &[("url", url.to_string())]).await
}

/// Full track objects for `ids`, in the order SoundCloud returns them.
pub async fn fetch_tracks(ids: &[u64]) -> Result<Vec<ScTrack>, String> {
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(TRACK_IDS_CHUNK) {
        let joined = chunk
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let tracks: Vec<serde_json::Value> = api_get("/tracks", &[("ids", joined)]).await?;
        out.extend(
            tracks
                .into_iter()
                .filter_map(|v| serde_json::from_value::<ScTrack>(v).ok()),
        );
    }
    Ok(out)
}

/// Replaces the stub tracks of a set with full objects, keeping set order. Stubs that
/// can't be loaded stay stubs (they're reported as unavailable later).
pub async fn hydrate_playlist_tracks(playlist: &mut ScPlaylist) -> Result<(), String> {
    let stub_ids: Vec<u64> = playlist
        .tracks
        .iter()
        .filter(|t| t.is_stub())
        .map(|t| t.id)
        .collect();
    if stub_ids.is_empty() {
        return Ok(());
    }
    let fetched = fetch_tracks(&stub_ids).await?;
    merge_hydrated_tracks(&mut playlist.tracks, fetched);
    Ok(())
}

pub fn merge_hydrated_tracks(tracks: &mut [ScTrack], fetched: Vec<ScTrack>) {
    let mut by_id: std::collections::HashMap<u64, ScTrack> =
        fetched.into_iter().map(|t| (t.id, t)).collect();
    for slot in tracks.iter_mut().filter(|t| t.is_stub()) {
        if let Some(full) = by_id.remove(&slot.id) {
            *slot = full;
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod helpers_tests;
