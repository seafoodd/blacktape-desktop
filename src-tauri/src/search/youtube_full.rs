//! YouTube Music side of the full search page (search v2): filtered, paged search + artist browse.
//!
//! OWNER: part B. Contract = v2\DECISIONS.md D1-D12 (all VERIFIED LIVE 2026-11-09). Do NOT edit
//! `youtube_fast.rs`; reuse its `pub` helpers (`CLIENT_VERSION`, `parse_count`, `parse_duration`) and copy
//! private ones. Same HTTP headers/client settings as youtube_fast (D1 of v1): Origin, Firefox UA,
//! `Cookie: SOCS=CAI`, body context WEB_REMIX / hl=en / gl=US; use your OWN static `OnceLock<reqwest::Client>`.
//!
//! Fixtures (tests use `include_str!`, no network): `fixtures/ytm_songs_page1_daft_punk.json`,
//! `ytm_songs_page2_daft_punk.json` (continuation response), `ytm_albums_page1_daft_punk.json`,
//! `ytm_artists_page1_john.json`, `ytm_artists_daft_punk_no_continuation.json`,
//! `ytm_artist_browse_daft_punk.json`, plus v1's `ytm_no_results.json`.

#![allow(dead_code)]

use crate::search::bandcamp::ItemType;
use crate::search::full::{ArtistProfile, SearchKind};
use crate::search::youtube_fast::{audience_from_segments, parse_count, parse_duration, CLIENT_VERSION};
use crate::search::SearchSuggestion;
use crate::types::Platform;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::error::Error;
use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// `params` of a type-filtered search (verified): songs / albums / artists.
pub const SONGS_PARAMS: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
pub const ALBUMS_PARAMS: &str = "EgWKAQIYAWoKEAkQBRAKEAMQBA==";
pub const ARTISTS_PARAMS: &str = "EgWKAQIgAWoKEAkQBRAKEAMQBA==";
/// Items per YouTube Music page (the filtered shelf returns 20; artists may return fewer, e.g. 11, with no token).
pub const PAGE_SIZE: usize = 20;
/// Hard stop for the first-page continuation chain (safety against loops): 5 requests.
pub const MAX_CHAIN_REQUESTS: usize = 5;

const SEARCH_URL: &str = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
const BROWSE_URL: &str = "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false";
const ORIGIN: &str = "https://music.youtube.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0";
const CONSENT_COOKIE: &str = "SOCS=CAI";
const WATCH_URL_PREFIX: &str = "https://youtube.com/watch?v=";
const ARTIST_URL_PREFIX: &str = "https://youtube.com/browse/";
const ALBUM_URL_PREFIX: &str = "https://music.youtube.com/browse/";
const SECTIONS_PATH: &str =
    "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
const BROWSE_SECTIONS_PATH: &str =
    "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
const SEPARATOR: &str = "•";
const IMG_SIZE_SUFFIX: &str = "=w544-h544";
/// Type labels (English, hl=en) that start a subtitle. Anything else at position 0 is not a label.
const KNOWN_LABELS: [&str; 10] = [
    "Song", "Album", "Single", "EP", "Artist", "Video", "Playlist", "Episode", "Podcast", "Profile",
];
/// Per-request timeout of search requests (first page and continuations).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
/// Total time budget of the first-page continuation chain (D18).
const FIRST_PAGE_BUDGET: Duration = Duration::from_secs(8);
/// Timeout of the artist browse request.
const BROWSE_TIMEOUT: Duration = Duration::from_secs(5);
/// "Top songs" rows shown on the artist card.
const TOP_SONGS_MAX: usize = 5;
/// Albums / singles shown on the artist card, per carousel.
const CAROUSEL_MAX: usize = 10;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// One parsed page: items in on-screen order plus the token for the next page (None = last page).
#[derive(Debug, Clone, Default)]
pub struct ParsedPage {
    pub items: Vec<SearchSuggestion>,
    pub next_token: Option<String>,
    /// Set when a later request of the first-page chain failed: the items collected so far are kept and
    /// `next_token` is the token that failed (so "Show more" retries it). Never set by `parse_page`.
    pub warning: Option<String>,
}

/// `params` constant for a kind.
pub fn params_for(kind: SearchKind) -> &'static str {
    match kind {
        SearchKind::Track => SONGS_PARAMS,
        SearchKind::Album => ALBUMS_PARAMS,
        SearchKind::Artist => ARTISTS_PARAMS,
    }
}

/// The InnerTube `context` object (identical for every request of this module).
fn context() -> Value {
    json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": CLIENT_VERSION,
            "hl": "en",
            "gl": "US"
        }
    })
}

/// Body of the first filtered request: same context as youtube_fast plus `"query"` and `"params"` (D1, D2). Pure.
pub fn build_search_body(query: &str, kind: SearchKind) -> Value {
    json!({
        "context": context(),
        "query": query,
        "params": params_for(kind),
    })
}

/// Body of a continuation request: only the context (D3). Pure.
pub fn build_continuation_body() -> Value {
    json!({ "context": context() })
}

/// Body of an artist browse request (D10). Pure.
pub fn build_browse_body(browse_id: &str) -> Value {
    json!({
        "context": context(),
        "browseId": browse_id,
    })
}

/// Url of a continuation request (D3): `https://music.youtube.com/youtubei/v1/search?ctoken={t}&continuation={t}&type=next&prettyPrint=false`
/// with the token inserted verbatim (it already contains `%3D`). The body is only the `context` (no query). Pure.
pub fn build_continuation_url(token: &str) -> String {
    format!(
        "https://music.youtube.com/youtubei/v1/search?ctoken={token}&continuation={token}&type=next&prettyPrint=false"
    )
}

/// Parses a first-page response (`contents.tabbedSearchResultsRenderer...sectionListRenderer.contents[].musicShelfRenderer`)
/// OR a continuation response (`continuationContents.musicShelfContinuation`) for `kind` (D4). Tolerant: garbage -> empty.
/// Row rules per kind (see DECISIONS D4 for exact strings): Track rows have NO type label (artist runs, album run, duration);
/// Album rows start with a label ("Album"/"Single"/"EP"); Artist rows "Artist • 67.4M monthly audience".
/// `next_token` from `continuations[0].nextContinuationData.continuation` of the shelf (first page: musicShelfRenderer,
/// continuation page: musicShelfContinuation). Field mapping equals v1 youtube_fast (D4/D9 there). Dedups by url.
pub fn parse_page(resp: &Value, kind: SearchKind) -> ParsedPage {
    let shelf = first_search_shelf(resp)
        .or_else(|| resp.pointer("/continuationContents/musicShelfContinuation"));
    let Some(shelf) = shelf else {
        return ParsedPage::default();
    };
    let items = shelf_rows(shelf)
        .filter_map(|row| parse_row(row, kind))
        .collect();
    ParsedPage {
        items: dedup_by_url(items),
        next_token: shelf_token(shelf),
        warning: None,
    }
}

/// First page (token None) or one continuation page (Some(token)), each request bounded by `REQUEST_TIMEOUT`.
pub async fn fetch_page(
    query: &str,
    kind: SearchKind,
    token: Option<&str>,
) -> Result<ParsedPage, Box<dyn Error + Send + Sync>> {
    fetch_page_within(query, kind, token, REQUEST_TIMEOUT).await
}

/// `fetch_page` with an explicit timeout (the first-page chain shrinks it to the remaining budget).
async fn fetch_page_within(
    query: &str,
    kind: SearchKind,
    token: Option<&str>,
    timeout: Duration,
) -> Result<ParsedPage, Box<dyn Error + Send + Sync>> {
    let request = match token {
        None => http_client()
            .post(SEARCH_URL)
            .json(&build_search_body(query, kind)),
        Some(token) => http_client()
            .post(build_continuation_url(token))
            .json(&build_continuation_body()),
    };
    let response = request
        .header("Origin", ORIGIN)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", CONSENT_COOKIE)
        .timeout(timeout)
        .send()
        .await?
        .error_for_status()?;
    let json: Value = response.json().await?;
    Ok(parse_page(&json, kind))
}

/// First-page chain (D5): `fetch_page(.., None)`, then follow `next_token` sequentially until `items.len() >= target`
/// or no token or `MAX_CHAIN_REQUESTS`. If the FIRST request fails -> Err; if a later one fails -> keep what was
/// collected and return the token that failed (so "Show more" can retry). Returns all collected items (not truncated)
/// and the last unused token. The whole chain shares `FIRST_PAGE_BUDGET` (8 s); each request gets the remaining budget
/// capped at `REQUEST_TIMEOUT`.
pub async fn fetch_first_page(
    query: &str,
    kind: SearchKind,
    target: usize,
) -> Result<ParsedPage, Box<dyn Error + Send + Sync>> {
    let first = fetch_page(query, kind, None).await?;
    let deadline = Instant::now() + FIRST_PAGE_BUDGET;
    let page = collect_chain(first, target, MAX_CHAIN_REQUESTS, |token: String| async move {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("first page time budget exhausted".to_string());
        }
        fetch_page_within(query, kind, Some(&token), remaining.min(REQUEST_TIMEOUT))
            .await
            .map_err(|e| e.to_string())
    })
    .await;
    Ok(page)
}

/// Follows continuation tokens after `first` until `items >= target`, no token, or `max_requests` requests in total
/// (the first request counts). `fetch_next` is called with the token to follow. On a failing request the collected
/// items are kept, `next_token` stays the failed token and `warning` holds the error. Items are deduplicated by url.
pub(crate) async fn collect_chain<F, Fut>(
    first: ParsedPage,
    target: usize,
    max_requests: usize,
    mut fetch_next: F,
) -> ParsedPage
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<ParsedPage, String>>,
{
    let mut items = first.items;
    let mut next = first.next_token;
    let mut warning: Option<String> = None;
    let mut requests = 1;
    while items.len() < target && requests < max_requests {
        let Some(token) = next.clone() else {
            break;
        };
        requests += 1;
        match fetch_next(token).await {
            Ok(page) => {
                items.extend(page.items);
                next = page.next_token;
            }
            Err(message) => {
                warning = Some(message);
                break;
            }
        }
    }
    ParsedPage {
        items: dedup_by_url(items),
        next_token: next,
        warning,
    }
}

/// `https://youtube.com/browse/UCxxxx` (or music.youtube.com / www.youtube.com) -> `UCxxxx`. None for anything else
/// (watch urls, album `MPREb_` ids, empty, no id, other hosts). Pure.
pub fn browse_id_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/')?;
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "youtube.com" | "www.youtube.com" | "music.youtube.com"
    ) {
        return None;
    }
    let id = path
        .strip_prefix("browse/")?
        .split(|c| matches!(c, '?' | '#' | '/'))
        .next()?;
    let valid = id.len() > 2
        && id.starts_with("UC")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    valid.then(|| id.to_string())
}

/// POST `https://music.youtube.com/youtubei/v1/browse?prettyPrint=false`, body = context + `"browseId": <UC id>` (D10).
/// Timeout 5 s. Parses with `parse_artist_profile`. Err when the url is not a YouTube artist url or the page has no name.
pub async fn artist_profile(url: &str) -> Result<ArtistProfile, Box<dyn Error + Send + Sync>> {
    let browse_id = browse_id_from_url(url).ok_or("not a YouTube artist url")?;
    let response = http_client()
        .post(BROWSE_URL)
        .header("Origin", ORIGIN)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", CONSENT_COOKIE)
        .json(&build_browse_body(&browse_id))
        .timeout(BROWSE_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    let json: Value = response.json().await?;
    let profile = parse_artist_profile(&json, &browse_id);
    if profile.name.is_empty() {
        return Err("YouTube artist page had no name".into());
    }
    Ok(profile)
}

/// Parses a browse response of an artist (D10 has the exact JSON paths). Tolerant: missing parts -> None/empty;
/// garbage -> default profile with empty name. `url` is the canonical artist url built from `browse_id`.
/// Shelves are recognised by title text ("Top songs", "Albums", "Singles & EPs"); all other shelves are ignored.
pub fn parse_artist_profile(resp: &Value, browse_id: &str) -> ArtistProfile {
    let header = resp.pointer("/header/musicImmersiveHeaderRenderer");
    let name = runs_text(runs(header.and_then(|h| h.pointer("/title/runs"))))
        .trim()
        .to_string();
    let subscribers = header
        .and_then(|h| {
            text_at(
                h,
                "/subscriptionButton/subscribeButtonRenderer/subscriberCountText/runs/0/text",
            )
            .or_else(|| {
                text_at(
                    h,
                    "/subscriptionButton/subscribeButtonRenderer/longSubscriberCountText/runs/0/text",
                )
            })
        })
        .and_then(parse_count);
    let monthly_audience = header
        .and_then(|h| text_at(h, "/monthlyListenerCount/runs/0/text"))
        .and_then(parse_count);
    let description = header
        .map(|h| runs_text(runs(h.pointer("/description/runs"))))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    // The header image is a wide banner; the widest (last) entry is kept.
    let banner = header
        .and_then(|h| h.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails"))
        .and_then(Value::as_array)
        .and_then(|thumbs| {
            thumbs
                .iter()
                .filter_map(|thumb| thumb.get("url").and_then(Value::as_str))
                .last()
        })
        .map(str::to_string);

    let sections: &[Value] = resp
        .pointer(BROWSE_SECTIONS_PATH)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut top_songs = Vec::new();
    let mut albums = Vec::new();
    let mut singles = Vec::new();
    for section in sections {
        if let Some(shelf) = section.get("musicShelfRenderer") {
            if top_songs.is_empty() && shelf_title(shelf) == Some("Top songs") {
                top_songs = parse_top_songs(shelf, &name);
            }
        } else if let Some(carousel) = section.get("musicCarouselShelfRenderer") {
            match carousel_title(carousel) {
                Some("Albums") if albums.is_empty() => albums = parse_carousel(carousel, &name),
                Some("Singles & EPs") if singles.is_empty() => {
                    singles = parse_carousel(carousel, &name)
                }
                _ => {}
            }
        }
    }

    ArtistProfile {
        platform: Some(Platform::Youtube),
        name,
        url: format!("{ARTIST_URL_PREFIX}{browse_id}"),
        avatar: None,
        banner,
        subscribers,
        monthly_audience,
        description,
        location: None,
        top_songs,
        albums,
        singles,
    }
}

// ---------------------------------------------------------------------------------------------
// Search row parsing
// ---------------------------------------------------------------------------------------------

/// One subtitle segment between " • " separators.
struct Segment {
    text: String,
    /// Texts of runs that carry a navigationEndpoint (linked artist names).
    linked: Vec<String>,
}

fn http_client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// `contents[]` of the first `musicShelfRenderer` of a first-page response.
fn first_search_shelf(resp: &Value) -> Option<&Value> {
    resp.pointer(SECTIONS_PATH)?
        .as_array()?
        .iter()
        .find_map(|section| section.get("musicShelfRenderer"))
}

/// The `musicResponsiveListItemRenderer` rows of a shelf (first page or continuation).
fn shelf_rows(shelf: &Value) -> impl Iterator<Item = &Value> + '_ {
    shelf
        .get("contents")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|entry| entry.get("musicResponsiveListItemRenderer"))
}

fn shelf_token(shelf: &Value) -> Option<String> {
    shelf
        .pointer("/continuations/0/nextContinuationData/continuation")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn parse_row(row: &Value, kind: SearchKind) -> Option<SearchSuggestion> {
    let title_runs = column_runs(row, 0);
    let title = runs_text(title_runs);
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let title_nav = title_runs.first().and_then(|run| run.get("navigationEndpoint"));
    let segments = segments(column_runs(row, 1));
    let (label, rest) = split_label(&segments);
    if !label_accepted(label, kind) {
        return None;
    }
    let img = best_thumbnail(row.get("thumbnail"));
    let browse_id = row
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .or_else(|| title_nav.and_then(|nav| nav.pointer("/browseEndpoint/browseId")))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());

    match kind {
        SearchKind::Track => {
            let video_id = row
                .pointer("/playlistItemData/videoId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .or_else(|| {
                    title_nav
                        .and_then(|nav| nav.pointer("/watchEndpoint/videoId"))
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                })?;
            Some(track(
                title,
                video_id,
                rest,
                parse_count(&runs_text(column_runs(row, 2))),
                img,
            ))
        }
        SearchKind::Album => {
            let browse_id = browse_id?;
            let artist = rest
                .iter()
                .find(|s| parse_year(&s.text).is_none())
                .map(display_artist)
                .unwrap_or_default();
            let year = rest.iter().rev().find_map(|s| parse_year(&s.text));
            Some(SearchSuggestion {
                item_type: ItemType::Album,
                name: title.to_string(),
                band_name: artist,
                album_name: None,
                item_url_path: format!("{ALBUM_URL_PREFIX}{browse_id}"),
                img,
                subscriber_count: None,
                audience_kind: None,
                view_count: None,
                year,
                duration: None,
                platform: Platform::Youtube,
            })
        }
        SearchKind::Artist => {
            let browse_id = browse_id?;
            let (subscriber_count, audience_kind) = audience_from_segments(rest.iter().map(|s| s.text.as_str()));
            Some(SearchSuggestion {
                item_type: ItemType::Artist,
                name: title.to_string(),
                band_name: "Artist".to_string(),
                album_name: None,
                item_url_path: format!("{ARTIST_URL_PREFIX}{browse_id}"),
                img,
                subscriber_count,
                audience_kind,
                view_count: None,
                year: None,
                duration: None,
                platform: Platform::Youtube,
            })
        }
    }
}

/// Builds a track from its subtitle segments after the label: artist, album (optional), duration (optional).
fn track(
    title: &str,
    video_id: &str,
    rest: &[Segment],
    view_count: Option<u64>,
    img: String,
) -> SearchSuggestion {
    let non_duration: Vec<&Segment> = rest
        .iter()
        .filter(|s| parse_duration(&s.text).is_none())
        .collect();
    SearchSuggestion {
        item_type: ItemType::Track,
        name: title.to_string(),
        band_name: non_duration
            .first()
            .map(|s| display_artist(s))
            .unwrap_or_default(),
        album_name: non_duration.get(1).map(|s| s.text.clone()),
        item_url_path: format!("{WATCH_URL_PREFIX}{video_id}"),
        img,
        subscriber_count: None,
        audience_kind: None,
        view_count,
        year: None,
        duration: rest.iter().find_map(|s| parse_duration(&s.text)),
        platform: Platform::Youtube,
    }
}

/// Splits a leading known type label ("Album", "Artist", ...) off the subtitle segments.
fn split_label(segments: &[Segment]) -> (Option<&str>, &[Segment]) {
    match segments.first() {
        Some(first) if KNOWN_LABELS.contains(&first.text.as_str()) => {
            (Some(first.text.as_str()), &segments[1..])
        }
        _ => (None, segments),
    }
}

/// Whether a row with this label (or none) can be of `kind`. Unlabelled rows are accepted (the kind is known).
fn label_accepted(label: Option<&str>, kind: SearchKind) -> bool {
    match (label, kind) {
        (None, _) => true,
        (Some("Song"), SearchKind::Track) => true,
        (Some("Album" | "Single" | "EP"), SearchKind::Album) => true,
        (Some("Artist"), SearchKind::Artist) => true,
        _ => false,
    }
}

/// Splits subtitle runs at " • " separators. Empty segments are dropped.
fn segments(runs: &[Value]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut current = Segment {
        text: String::new(),
        linked: Vec::new(),
    };
    for run in runs {
        let text = run.get("text").and_then(Value::as_str).unwrap_or("");
        if text.trim() == SEPARATOR {
            out.push(std::mem::replace(
                &mut current,
                Segment {
                    text: String::new(),
                    linked: Vec::new(),
                },
            ));
            continue;
        }
        current.text.push_str(text);
        let trimmed = text.trim();
        if !trimmed.is_empty() && run.get("navigationEndpoint").is_some() {
            current.linked.push(trimmed.to_string());
        }
    }
    out.push(current);

    out.into_iter()
        .map(|mut segment| {
            segment.text = segment.text.trim().to_string();
            segment
        })
        .filter(|segment| !segment.text.is_empty())
        .collect()
}

/// Several linked artists are joined with ", "; otherwise the segment text is shown as-is.
fn display_artist(segment: &Segment) -> String {
    if segment.linked.len() >= 2 {
        segment.linked.join(", ")
    } else {
        segment.text.clone()
    }
}

fn parse_year(text: &str) -> Option<i32> {
    let t = text.trim();
    if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
        t.parse().ok()
    } else {
        None
    }
}

/// Picks the widest thumbnail and upscales Google image urls.
fn best_thumbnail(container: Option<&Value>) -> String {
    let Some(thumbs) = container
        .and_then(|c| c.pointer("/musicThumbnailRenderer/thumbnail/thumbnails"))
        .and_then(Value::as_array)
    else {
        return String::new();
    };
    thumbs
        .iter()
        .filter_map(|thumb| {
            let url = thumb.get("url")?.as_str()?;
            let width = thumb.get("width").and_then(Value::as_u64).unwrap_or(0);
            Some((width, url))
        })
        .max_by_key(|(width, _)| *width)
        .map(|(_, url)| upscale_img(url))
        .unwrap_or_default()
}

/// Rewrites `...=w60-h60<rest>` to `...=w544-h544<rest>` for Google image hosts. Other urls unchanged.
fn upscale_img(url: &str) -> String {
    let is_google_img = url.contains("lh3.googleusercontent.com")
        || url.contains("yt3.googleusercontent.com");
    if !is_google_img {
        return url.to_string();
    }
    let Some(idx) = url.rfind("=w") else {
        return url.to_string();
    };
    let tail = &url[idx + 2..];
    let width_len = tail.bytes().take_while(u8::is_ascii_digit).count();
    let Some(after_width) = tail[width_len..].strip_prefix("-h") else {
        return url.to_string();
    };
    let height_len = after_width.bytes().take_while(u8::is_ascii_digit).count();
    if width_len == 0 || height_len == 0 {
        return url.to_string();
    }
    format!(
        "{}{}{}",
        &url[..idx],
        IMG_SIZE_SUFFIX,
        &after_width[height_len..]
    )
}

/// Keeps the first item for each item url.
fn dedup_by_url(items: Vec<SearchSuggestion>) -> Vec<SearchSuggestion> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.item_url_path.clone()))
        .collect()
}

fn column_runs(row: &Value, index: usize) -> &[Value] {
    runs(row.pointer(&format!(
        "/flexColumns/{index}/musicResponsiveListItemFlexColumnRenderer/text/runs"
    )))
}

fn runs(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn text_of(run: &Value) -> String {
    run.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn runs_text(runs: &[Value]) -> String {
    runs.iter().map(text_of).collect()
}

fn text_at<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

// ---------------------------------------------------------------------------------------------
// Artist profile parsing
// ---------------------------------------------------------------------------------------------

fn shelf_title(shelf: &Value) -> Option<&str> {
    text_at(shelf, "/title/runs/0/text")
}

fn carousel_title(carousel: &Value) -> Option<&str> {
    text_at(
        carousel,
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
    )
}

/// "Top songs" shelf rows: title col0 (videoId), artists col1, plays col2, album col3; no duration.
fn parse_top_songs(shelf: &Value, fallback_artist: &str) -> Vec<SearchSuggestion> {
    let mut out = Vec::new();
    for row in shelf_rows(shelf) {
        if out.len() >= TOP_SONGS_MAX {
            break;
        }
        let title_runs = column_runs(row, 0);
        let title = runs_text(title_runs);
        let title = title.trim();
        let title_nav = title_runs.first().and_then(|run| run.get("navigationEndpoint"));
        let video_id = row
            .pointer("/playlistItemData/videoId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .or_else(|| {
                title_nav
                    .and_then(|nav| nav.pointer("/watchEndpoint/videoId"))
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
            });
        let Some(video_id) = video_id.filter(|_| !title.is_empty()) else {
            continue;
        };
        let artist = segments(column_runs(row, 1))
            .first()
            .map(display_artist)
            .filter(|artist| !artist.is_empty())
            .unwrap_or_else(|| fallback_artist.to_string());
        let album = runs_text(column_runs(row, 3)).trim().to_string();
        out.push(SearchSuggestion {
            item_type: ItemType::Track,
            name: title.to_string(),
            band_name: artist,
            album_name: (!album.is_empty()).then_some(album),
            item_url_path: format!("{WATCH_URL_PREFIX}{video_id}"),
            img: best_thumbnail(row.get("thumbnail")),
            subscriber_count: None,
            audience_kind: None,
            view_count: parse_count(&runs_text(column_runs(row, 2))),
            year: None,
            duration: None,
            platform: Platform::Youtube,
        });
    }
    dedup_by_url(out)
}

/// "Albums" / "Singles & EPs" carousel: `musicTwoRowItemRenderer` items, album kind, year from the subtitle.
fn parse_carousel(carousel: &Value, fallback_artist: &str) -> Vec<SearchSuggestion> {
    let Some(items) = carousel.get("contents").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        if out.len() >= CAROUSEL_MAX {
            break;
        }
        let Some(two_row) = item.get("musicTwoRowItemRenderer") else {
            continue;
        };
        let title_run = two_row.pointer("/title/runs/0");
        let name = title_run.map(text_of).unwrap_or_default();
        let name = name.trim().to_string();
        let browse_id = title_run
            .and_then(|run| run.pointer("/navigationEndpoint/browseEndpoint/browseId"))
            .or_else(|| two_row.pointer("/navigationEndpoint/browseEndpoint/browseId"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let (false, Some(browse_id)) = (name.is_empty(), browse_id) else {
            continue;
        };
        let year = segments(runs(two_row.pointer("/subtitle/runs")))
            .iter()
            .rev()
            .find_map(|s| parse_year(&s.text));
        out.push(SearchSuggestion {
            item_type: ItemType::Album,
            name,
            band_name: fallback_artist.to_string(),
            album_name: None,
            item_url_path: format!("{ALBUM_URL_PREFIX}{browse_id}"),
            img: best_thumbnail(two_row.get("thumbnailRenderer")),
            subscriber_count: None,
            audience_kind: None,
            view_count: None,
            year,
            duration: None,
            platform: Platform::Youtube,
        });
    }
    dedup_by_url(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    const SONGS_P1: &str = include_str!("fixtures/ytm_songs_page1_daft_punk.json");
    const SONGS_P2: &str = include_str!("fixtures/ytm_songs_page2_daft_punk.json");
    const ALBUMS_P1: &str = include_str!("fixtures/ytm_albums_page1_daft_punk.json");
    const ARTISTS_JOHN: &str = include_str!("fixtures/ytm_artists_page1_john.json");
    const ARTISTS_DP: &str = include_str!("fixtures/ytm_artists_daft_punk_no_continuation.json");
    const NO_RESULTS: &str = include_str!("fixtures/ytm_no_results.json");
    const BROWSE_DP: &str = include_str!("fixtures/ytm_artist_browse_daft_punk.json");

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is valid json")
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    /// Rows of the first search shelf of a mutable first-page response.
    fn shelf_rows_mut(resp: &mut Value) -> &mut Vec<Value> {
        resp.pointer_mut(
            "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicShelfRenderer/contents",
        )
        .and_then(Value::as_array_mut)
        .expect("shelf rows")
    }

    fn suggestion(name: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type: ItemType::Track,
            name: name.to_string(),
            band_name: "band".to_string(),
            album_name: None,
            item_url_path: format!("https://youtube.com/watch?v={name}"),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    fn page_of(names: &[&str], next: Option<&str>) -> ParsedPage {
        ParsedPage {
            items: names.iter().map(|n| suggestion(n)).collect(),
            next_token: next.map(str::to_string),
            warning: None,
        }
    }

    fn names(items: &[SearchSuggestion]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn params_per_kind() {
        assert_eq!(params_for(SearchKind::Track), SONGS_PARAMS);
        assert_eq!(params_for(SearchKind::Album), ALBUMS_PARAMS);
        assert_eq!(params_for(SearchKind::Artist), ARTISTS_PARAMS);
    }

    #[test]
    fn fixtures_are_valid_json() {
        for f in [
            SONGS_P1,
            SONGS_P2,
            ALBUMS_P1,
            ARTISTS_JOHN,
            ARTISTS_DP,
            BROWSE_DP,
            NO_RESULTS,
        ] {
            serde_json::from_str::<Value>(f).unwrap();
        }
    }

    // ----- request builders -----

    #[test]
    fn search_body_has_query_params_and_context() {
        for kind in [SearchKind::Track, SearchKind::Album, SearchKind::Artist] {
            let body = build_search_body("daft punk", kind);
            assert_eq!(body["query"], "daft punk");
            assert_eq!(body["params"], params_for(kind));
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["hl"], "en");
            assert_eq!(body["context"]["client"]["gl"], "US");
        }
    }

    #[test]
    fn search_body_keeps_quotes_backslashes_and_unicode() {
        let query = "h\u{e9}llo \"x\" \\ \u{65e5}\u{672c}";
        let body = build_search_body(query, SearchKind::Track);
        assert_eq!(body["query"].as_str(), Some(query));
        let text = serde_json::to_string(&body).unwrap();
        assert!(serde_json::from_str::<Value>(&text).is_ok());
    }

    #[test]
    fn continuation_url_uses_token_verbatim() {
        let token = "EqYDabc%3D%3D";
        let url = build_continuation_url(token);
        assert_eq!(
            url,
            "https://music.youtube.com/youtubei/v1/search?ctoken=EqYDabc%3D%3D&continuation=EqYDabc%3D%3D&type=next&prettyPrint=false"
        );
        assert!(!url.contains("%253D"), "token must not be encoded twice");
    }

    #[test]
    fn continuation_body_is_context_only() {
        let body = build_continuation_body();
        let keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["context"]);
    }

    #[test]
    fn browse_body_carries_browse_id() {
        let body = build_browse_body("UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(body["browseId"], "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
    }

    // ----- search row parsing -----

    #[test]
    fn songs_page1_parses_tracks_and_token() {
        let page = parse_page(&fixture(SONGS_P1), SearchKind::Track);
        assert_eq!(page.items.len(), 5);
        assert!(page.items.iter().all(|i| i.item_type == ItemType::Track));
        let first = &page.items[0];
        assert_eq!(first.name, "Instant Crush (feat. Julian Casablancas)");
        assert_eq!(first.band_name, "Daft Punk, Julian Casablancas");
        assert_eq!(first.album_name.as_deref(), Some("Random Access Memories"));
        assert_eq!(first.duration, Some(338));
        assert_eq!(first.view_count, Some(1_200_000_000));
        assert_eq!(first.item_url_path, "https://youtube.com/watch?v=khnokW3Mw24");
        assert_eq!(first.platform, Platform::Youtube);
        assert_eq!(first.year, None);
        let token = page.next_token.expect("page 1 has a continuation");
        assert!(token.contains("%3D"), "token keeps its escapes");
        assert!(page.warning.is_none());
    }

    #[test]
    fn songs_row_fields_album_duration_and_plays() {
        let page = parse_page(&fixture(SONGS_P1), SearchKind::Track);
        let row = &page.items[1];
        assert_eq!(row.name, "Around the World");
        assert_eq!(row.band_name, "Daft Punk");
        assert_eq!(row.album_name.as_deref(), Some("Homework"));
        assert_eq!(row.duration, Some(430));
        assert_eq!(row.view_count, Some(301_000_000));
    }

    #[test]
    fn songs_multiple_linked_artists_are_joined() {
        let page = parse_page(&fixture(SONGS_P1), SearchKind::Track);
        let row = &page.items[2];
        assert_eq!(row.name, "Get Lucky (feat. Pharrell Williams and Nile Rodgers)");
        assert_eq!(row.band_name, "Daft Punk, Pharrell Williams, Nile Rodgers");
        assert_eq!(row.album_name.as_deref(), Some("Random Access Memories"));
        assert_eq!(row.duration, Some(370));
    }

    #[test]
    fn songs_page2_continuation_shape_parses() {
        let page = parse_page(&fixture(SONGS_P2), SearchKind::Track);
        assert_eq!(page.items.len(), 4);
        let first = &page.items[0];
        assert_eq!(first.name, "I Feel It Coming (feat. Daft Punk)");
        assert_eq!(first.band_name, "The Weeknd");
        assert_eq!(first.album_name.as_deref(), Some("Starboy"));
        assert_eq!(first.duration, Some(270));
        assert!(page.next_token.is_some());
    }

    #[test]
    fn albums_page1_parses_albums() {
        let page = parse_page(&fixture(ALBUMS_P1), SearchKind::Album);
        assert_eq!(page.items.len(), 5);
        let first = &page.items[0];
        assert_eq!(first.item_type, ItemType::Album);
        assert_eq!(first.name, "Random Access Memories");
        assert_eq!(first.band_name, "Daft Punk");
        assert_eq!(first.year, Some(2013));
        assert_eq!(first.duration, None);
        assert_eq!(first.view_count, None);
        assert_eq!(
            first.item_url_path,
            "https://music.youtube.com/browse/MPREb_K8qWMWVqXGi"
        );
        assert!(page.next_token.is_some());
    }

    #[test]
    fn album_thumbnail_is_upscaled() {
        let page = parse_page(&fixture(ALBUMS_P1), SearchKind::Album);
        let img = &page.items[0].img;
        assert!(img.starts_with("https://yt3.googleusercontent.com/"), "{img}");
        assert!(img.contains("=w544-h544"), "{img}");
        assert!(!img.contains("=w120-h120"), "{img}");
    }

    #[test]
    fn artists_page1_parses_artists_with_token() {
        let page = parse_page(&fixture(ARTISTS_JOHN), SearchKind::Artist);
        assert_eq!(page.items.len(), 5);
        let first = &page.items[0];
        assert_eq!(first.item_type, ItemType::Artist);
        assert_eq!(first.name, "john");
        assert_eq!(first.band_name, "Artist");
        assert_eq!(first.subscriber_count, Some(2_260_000));
        assert_eq!(first.item_url_path, "https://youtube.com/browse/UCLI_vItz-HMYnbh1OmKCD6w");
        assert!(page.next_token.is_some());
    }

    #[test]
    fn artists_without_continuation_have_no_token() {
        let page = parse_page(&fixture(ARTISTS_DP), SearchKind::Artist);
        assert_eq!(page.items.len(), 4);
        assert!(page.next_token.is_none());
        assert_eq!(page.items[0].name, "Daft Punk");
        assert_eq!(page.items[0].subscriber_count, Some(67_400_000));
    }

    #[test]
    fn no_results_fixture_is_empty_for_every_kind() {
        for kind in [SearchKind::Track, SearchKind::Album, SearchKind::Artist] {
            let page = parse_page(&fixture(NO_RESULTS), kind);
            assert!(page.items.is_empty());
            assert!(page.next_token.is_none());
        }
    }

    #[test]
    fn garbage_shapes_are_empty() {
        let shapes = [
            Value::Null,
            json!({}),
            json!([]),
            json!("text"),
            json!({"contents": 5}),
            json!({"continuationContents": {"musicShelfContinuation": "x"}}),
            json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": []}}}),
        ];
        for shape in &shapes {
            let page = parse_page(shape, SearchKind::Track);
            assert!(page.items.is_empty());
            assert!(page.next_token.is_none());
        }
    }

    #[test]
    fn malformed_rows_are_skipped() {
        let mut resp = fixture(SONGS_P1);
        let rows = shelf_rows_mut(&mut resp);
        rows.clear();
        rows.push(json!({"foo": 1}));
        rows.push(json!({"musicResponsiveListItemRenderer": {}}));
        rows.push(json!({"musicResponsiveListItemRenderer": {"flexColumns": 5}}));
        let page = parse_page(&resp, SearchKind::Track);
        assert!(page.items.is_empty());
    }

    #[test]
    fn duplicate_rows_are_deduplicated() {
        let mut resp = fixture(SONGS_P1);
        let rows = shelf_rows_mut(&mut resp);
        let first = rows[0].clone();
        rows.push(first);
        let page = parse_page(&resp, SearchKind::Track);
        assert_eq!(page.items.len(), 5);
    }

    #[test]
    fn track_missing_album_and_duration() {
        let mut resp = fixture(SONGS_P1);
        let rows = shelf_rows_mut(&mut resp);
        rows.clear();
        rows.push(json!({"musicResponsiveListItemRenderer": {
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Solo"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Daft Punk", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCRr1xG_2WIDs18a6cIiCxeA"}}}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "5M plays"}]}}}
            ],
            "playlistItemData": {"videoId": "abc123"}
        }}));
        let page = parse_page(&resp, SearchKind::Track);
        assert_eq!(page.items.len(), 1);
        let item = &page.items[0];
        assert_eq!(item.band_name, "Daft Punk");
        assert_eq!(item.album_name, None);
        assert_eq!(item.duration, None);
        assert_eq!(item.view_count, Some(5_000_000));
        assert_eq!(item.item_url_path, "https://youtube.com/watch?v=abc123");
    }

    #[test]
    fn unicode_title_is_kept() {
        let mut resp = fixture(SONGS_P1);
        let rows = shelf_rows_mut(&mut resp);
        rows.clear();
        rows.push(json!({"musicResponsiveListItemRenderer": {
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "\u{c7}a plane pour moi \u{2013} \u{65e5}\u{672c}\u{8a9e} \u{266a}"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "\u{30c6}\u{30b9}\u{30c8}"}, {"text": " \u{2022} "}, {"text": "3:00"}]}}}
            ],
            "playlistItemData": {"videoId": "uni"}
        }}));
        let page = parse_page(&resp, SearchKind::Track);
        assert_eq!(page.items[0].name, "\u{c7}a plane pour moi \u{2013} \u{65e5}\u{672c}\u{8a9e} \u{266a}");
        assert_eq!(page.items[0].band_name, "\u{30c6}\u{30b9}\u{30c8}");
        assert_eq!(page.items[0].duration, Some(180));
    }

    #[test]
    fn rows_without_identifiers_are_skipped() {
        let mut resp = fixture(SONGS_P1);
        let rows = shelf_rows_mut(&mut resp);
        rows.clear();
        // Track without video id.
        rows.push(json!({"musicResponsiveListItemRenderer": {"flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "NoId"}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Band"}]}}}
        ]}}));
        // Empty title.
        rows.push(json!({"musicResponsiveListItemRenderer": {"flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "   "}]}}}
        ], "playlistItemData": {"videoId": "x"}}}));
        let page = parse_page(&resp, SearchKind::Track);
        assert!(page.items.is_empty());

        // Album without browse id, artist without browse id.
        let mut album = fixture(ALBUMS_P1);
        let album_rows = shelf_rows_mut(&mut album);
        album_rows.clear();
        album_rows.push(json!({"musicResponsiveListItemRenderer": {"flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "NoBrowse"}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Album"}, {"text": " \u{2022} "}, {"text": "Daft Punk"}]}}}
        ]}}));
        assert!(parse_page(&album, SearchKind::Album).items.is_empty());

        let mut artist = fixture(ARTISTS_DP);
        let artist_rows = shelf_rows_mut(&mut artist);
        artist_rows.clear();
        artist_rows.push(json!({"musicResponsiveListItemRenderer": {"flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "NoBrowse"}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Artist"}, {"text": " \u{2022} "}, {"text": "1K subscribers"}]}}}
        ]}}));
        assert!(parse_page(&artist, SearchKind::Artist).items.is_empty());
    }

    #[test]
    fn wrong_type_label_is_skipped() {
        let mut album = fixture(ALBUMS_P1);
        let rows = shelf_rows_mut(&mut album);
        rows.clear();
        rows.push(json!({"musicResponsiveListItemRenderer": {
            "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_x"}},
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Playlist thing"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Playlist"}, {"text": " \u{2022} "}, {"text": "Someone"}]}}}
            ]
        }}));
        assert!(parse_page(&album, SearchKind::Album).items.is_empty());
    }

    // ----- continuation chain -----

    #[test]
    fn chain_stops_when_target_reached() {
        let calls = RefCell::new(Vec::new());
        let page = block_on(collect_chain(
            page_of(&["a", "b"], Some("t1")),
            2,
            5,
            |token: String| {
                calls.borrow_mut().push(token);
                async { Err::<ParsedPage, String>("must not be called".into()) }
            },
        ));
        assert!(calls.borrow().is_empty());
        assert_eq!(names(&page.items), vec!["a", "b"]);
        assert_eq!(page.next_token.as_deref(), Some("t1"));
        assert!(page.warning.is_none());
    }

    #[test]
    fn chain_follows_tokens_until_target() {
        let calls = RefCell::new(Vec::new());
        let script = RefCell::new(VecDeque::from(vec![Ok(page_of(&["c", "d"], Some("t2")))]));
        let page = block_on(collect_chain(
            page_of(&["a", "b"], Some("t1")),
            3,
            5,
            |token: String| {
                calls.borrow_mut().push(token);
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        assert_eq!(calls.borrow().as_slice(), ["t1"]);
        assert_eq!(names(&page.items), vec!["a", "b", "c", "d"]);
        assert_eq!(page.next_token.as_deref(), Some("t2"));
        assert!(page.warning.is_none());
    }

    #[test]
    fn chain_ends_when_no_token_is_left() {
        let calls = RefCell::new(Vec::new());
        let script = RefCell::new(VecDeque::from(vec![Ok(page_of(&["b"], None))]));
        let page = block_on(collect_chain(
            page_of(&["a"], Some("t1")),
            50,
            5,
            |token: String| {
                calls.borrow_mut().push(token);
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        assert_eq!(names(&page.items), vec!["a", "b"]);
        assert!(page.next_token.is_none());
        assert_eq!(calls.borrow().len(), 1);
    }

    #[test]
    fn chain_without_first_token_makes_no_request() {
        let calls = RefCell::new(0usize);
        let page = block_on(collect_chain(page_of(&["a"], None), 50, 5, |_t: String| {
            *calls.borrow_mut() += 1;
            async { Err::<ParsedPage, String>("x".into()) }
        }));
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(names(&page.items), vec!["a"]);
        assert!(page.next_token.is_none());
    }

    #[test]
    fn chain_failure_keeps_items_and_failed_token() {
        let script = RefCell::new(VecDeque::from(vec![Err("boom".to_string())]));
        let page = block_on(collect_chain(
            page_of(&["a"], Some("t1")),
            50,
            5,
            |_t: String| {
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        assert_eq!(names(&page.items), vec!["a"]);
        assert_eq!(page.next_token.as_deref(), Some("t1"));
        assert_eq!(page.warning.as_deref(), Some("boom"));
    }

    #[test]
    fn chain_second_request_failure_returns_second_token() {
        let calls = RefCell::new(Vec::new());
        let script = RefCell::new(VecDeque::from(vec![
            Ok(page_of(&["b"], Some("t2"))),
            Err("late failure".to_string()),
        ]));
        let page = block_on(collect_chain(
            page_of(&["a"], Some("t1")),
            50,
            5,
            |token: String| {
                calls.borrow_mut().push(token);
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        assert_eq!(calls.borrow().as_slice(), ["t1", "t2"]);
        assert_eq!(names(&page.items), vec!["a", "b"]);
        assert_eq!(page.next_token.as_deref(), Some("t2"));
        assert_eq!(page.warning.as_deref(), Some("late failure"));
    }

    #[test]
    fn chain_respects_max_requests() {
        let calls = RefCell::new(Vec::new());
        let script = RefCell::new(VecDeque::from(vec![
            Ok(page_of(&["b"], Some("t2"))),
            Ok(page_of(&["c"], Some("t3"))),
            Ok(page_of(&["d"], Some("t4"))),
        ]));
        let page = block_on(collect_chain(
            page_of(&["a"], Some("t1")),
            1000,
            3,
            |token: String| {
                calls.borrow_mut().push(token);
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        // First request counts: 3 in total means two continuation requests.
        assert_eq!(calls.borrow().as_slice(), ["t1", "t2"]);
        assert_eq!(names(&page.items), vec!["a", "b", "c"]);
        assert_eq!(page.next_token.as_deref(), Some("t3"));
    }

    #[test]
    fn chain_dedups_items_across_pages() {
        let script = RefCell::new(VecDeque::from(vec![Ok(page_of(&["a", "b"], None))]));
        let page = block_on(collect_chain(
            page_of(&["a"], Some("t1")),
            50,
            5,
            |_t: String| {
                let next = script.borrow_mut().pop_front().expect("unexpected request");
                async move { next }
            },
        ));
        assert_eq!(names(&page.items), vec!["a", "b"]);
    }

    // ----- browse id -----

    #[test]
    fn browse_id_from_artist_urls() {
        assert_eq!(
            browse_id_from_url("https://youtube.com/browse/UCRr1xG_2WIDs18a6cIiCxeA").as_deref(),
            Some("UCRr1xG_2WIDs18a6cIiCxeA")
        );
        assert_eq!(
            browse_id_from_url("https://music.youtube.com/browse/UCabc-_1?foo=1").as_deref(),
            Some("UCabc-_1")
        );
        assert_eq!(
            browse_id_from_url("  https://www.youtube.com/browse/UCabc/ ").as_deref(),
            Some("UCabc")
        );
    }

    #[test]
    fn browse_id_rejects_other_urls() {
        for url in [
            "",
            "   ",
            "https://youtube.com/watch?v=abc",
            "https://youtube.com/browse/",
            "https://youtube.com/browse/UC",
            "https://youtube.com/browse/MPREb_K8qWMWVqXGi",
            "https://example.com/browse/UCabc",
            "https://youtube.com.evil.com/browse/UCabc",
            "https://youtube.com/browse/UC%20bad",
            "UCabc",
            "ftp://youtube.com/browse/UCabc",
            "https://youtube.com",
        ] {
            assert_eq!(browse_id_from_url(url), None, "url {url:?}");
        }
    }

    #[test]
    fn artist_profile_rejects_non_artist_url_without_network() {
        let result = block_on(artist_profile("https://youtube.com/watch?v=abc"));
        assert!(result.is_err());
    }

    // ----- artist profile -----

    #[test]
    fn profile_header_fields() {
        let profile = parse_artist_profile(&fixture(BROWSE_DP), "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(profile.name, "Daft Punk");
        assert_eq!(profile.platform, Some(Platform::Youtube));
        assert_eq!(profile.url, "https://youtube.com/browse/UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(profile.subscribers, Some(7_210_000));
        assert_eq!(profile.monthly_audience, Some(67_400_000));
        assert!(profile
            .description
            .as_deref()
            .unwrap_or("")
            .starts_with("Daft Punk were a French electronic music duo"));
        assert_eq!(profile.avatar, None);
        assert_eq!(profile.location, None);
        let banner = profile.banner.expect("banner");
        assert!(banner.contains("w1080-h450"), "{banner}");
    }

    #[test]
    fn profile_top_songs_capped_and_parsed() {
        let profile = parse_artist_profile(&fixture(BROWSE_DP), "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(profile.top_songs.len(), 5);
        let first = &profile.top_songs[0];
        assert_eq!(first.item_type, ItemType::Track);
        assert_eq!(first.name, "Instant Crush (feat. Julian Casablancas)");
        assert_eq!(first.band_name, "Daft Punk, Julian Casablancas");
        assert_eq!(first.album_name.as_deref(), Some("Random Access Memories"));
        assert_eq!(first.view_count, Some(1_200_000_000));
        assert_eq!(first.duration, None);
        assert_eq!(first.item_url_path, "https://youtube.com/watch?v=khnokW3Mw24");
        assert_eq!(profile.top_songs[3].album_name.as_deref(), Some("Homework"));
    }

    #[test]
    fn profile_albums_and_singles_with_years() {
        let profile = parse_artist_profile(&fixture(BROWSE_DP), "UCRr1xG_2WIDs18a6cIiCxeA");
        assert_eq!(profile.albums.len(), 4);
        let first = &profile.albums[0];
        assert_eq!(first.item_type, ItemType::Album);
        assert_eq!(first.name, "Random Access Memories (Drumless Edition)");
        assert_eq!(first.year, Some(2023));
        assert_eq!(first.band_name, "Daft Punk");
        assert_eq!(first.item_url_path, "https://music.youtube.com/browse/MPREb_OgeEnoHTsCm");
        assert!(first.img.contains("=w544-h544"), "{}", first.img);

        assert_eq!(profile.singles.len(), 4);
        let years: Vec<Option<i32>> = profile.singles.iter().map(|s| s.year).collect();
        assert_eq!(years, vec![Some(2023), Some(2023), Some(2006), Some(2005)]);
        assert_eq!(profile.singles[0].name, "GLBTM (Studio Outtakes)");
        assert!(profile.singles.iter().all(|s| s.item_type == ItemType::Album));
        assert!(profile.singles.iter().all(|s| s.band_name == "Daft Punk"));
    }

    #[test]
    fn profile_ignores_unrelated_shelves() {
        let resp = json!({
            "header": {"musicImmersiveHeaderRenderer": {"title": {"runs": [{"text": "X"}]}}},
            "contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
                {"musicCarouselShelfRenderer": {
                    "header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Videos"}]}}},
                    "contents": [{"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Clip", "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_v"}}}]}}}]
                }},
                {"musicShelfRenderer": {"title": {"runs": [{"text": "Live performances"}]}, "contents": [
                    {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Live"}]}}}], "playlistItemData": {"videoId": "live1"}}}
                ]}}
            ]}}}}]}}
        });
        let profile = parse_artist_profile(&resp, "UC1");
        assert_eq!(profile.name, "X");
        assert!(profile.albums.is_empty());
        assert!(profile.singles.is_empty());
        assert!(profile.top_songs.is_empty());
    }

    #[test]
    fn profile_garbage_gives_empty_name_and_no_panic() {
        for shape in [Value::Null, json!({}), json!([1, 2]), json!({"header": 3})] {
            let profile = parse_artist_profile(&shape, "UC1");
            assert_eq!(profile.name, "");
            assert_eq!(profile.subscribers, None);
            assert_eq!(profile.monthly_audience, None);
            assert_eq!(profile.description, None);
            assert_eq!(profile.banner, None);
            assert!(profile.top_songs.is_empty());
            assert!(profile.albums.is_empty());
            assert!(profile.singles.is_empty());
        }
    }

    #[test]
    fn profile_unicode_name_is_kept() {
        let resp = json!({"header": {"musicImmersiveHeaderRenderer": {
            "title": {"runs": [{"text": "  \u{d8}ystein \u{c5}sen  "}]},
            "monthlyListenerCount": {"runs": [{"text": "1.5K monthly audience"}]}
        }}});
        let profile = parse_artist_profile(&resp, "UC1");
        assert_eq!(profile.name, "\u{d8}ystein \u{c5}sen");
        assert_eq!(profile.monthly_audience, Some(1_500));
    }
}
