//! Fast YouTube Music search over the InnerTube API (WEB_REMIX client).
//!
//! OWNER: part 1 (youtube_fast). Nobody else edits this file or `fixtures/`.
//! See C:\Dev\blacktape-search-work\DECISIONS.md (D1-D8) for the verified request shape.
//!
//! Request (verified live, no API key / visitorData / cookies required):
//!   POST https://music.youtube.com/youtubei/v1/search?prettyPrint=false
//!   headers: Content-Type: application/json, Origin: https://music.youtube.com,
//!            User-Agent: <browser UA>, Cookie: SOCS=CAI (EU consent bypass)
//!   body: {"context":{"client":{"clientName":"WEB_REMIX","clientVersion":CLIENT_VERSION,
//!          "hl":"en","gl":"US"}},"query":Q}                       (unfiltered)
//!   body + ,"params":SONGS_FILTER_PARAMS                           (songs-only)

#![allow(dead_code)]

use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::error::Error;
use std::sync::OnceLock;
use std::time::Duration;

/// WEB_REMIX client version sent to InnerTube. Old versions keep working, so this is just a constant.
pub const CLIENT_VERSION: &str = "1.20261104.00.00";
/// `params` value that restricts a search to songs (returns album name + duration per row).
pub const SONGS_FILTER_PARAMS: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";

const SEARCH_URL: &str = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
const ORIGIN: &str = "https://music.youtube.com";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0";
const CONSENT_COOKIE: &str = "SOCS=CAI";
const WATCH_URL_PREFIX: &str = "https://youtube.com/watch?v=";
const ARTIST_URL_PREFIX: &str = "https://youtube.com/browse/";
const ALBUM_URL_PREFIX: &str = "https://music.youtube.com/browse/";
const SECTIONS_PATH: &str =
    "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
const SEPARATOR: &str = "•";
const IMG_SIZE_SUFFIX: &str = "=w544-h544";
/// Type labels (English, hl=en) that start a subtitle. Anything else at position 0 is not a label.
const KNOWN_LABELS: [&str; 10] = [
    "Song", "Album", "Single", "EP", "Artist", "Video", "Playlist", "Episode", "Podcast", "Profile",
];

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Public entry point used by `search::search_youtube_with_fallback`.
///
/// Contract:
/// * Sends TWO requests concurrently over one shared static `reqwest::Client` (OnceLock; keep-alive,
///   gzip/brotli, connect_timeout 2s, per-request timeout 3s): the unfiltered one and the songs-filtered one.
/// * Parses both with `parse_search_response`, combines them with `merge_results`.
/// * If the UNFILTERED request fails (network, non-2xx, invalid JSON) -> `Err`.
///   If only the songs-filtered request fails -> ignore it (use unfiltered only).
/// * Returns `Ok(vec)` (possibly empty). The caller treats empty as "fall back".
/// * Never panics, never blocks the runtime (async reqwest only).
pub async fn search(query: &str) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let client = http_client();
    let (unfiltered, songs) = tokio::join!(
        fetch_once(client, query, false),
        fetch_once(client, query, true)
    );
    combine_outcomes(unfiltered, songs)
}

fn http_client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

async fn fetch_once(
    client: &reqwest::Client,
    query: &str,
    songs_only: bool,
) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let body = build_request_body(query, songs_only);
    let response = client
        .post(SEARCH_URL)
        .header("Origin", ORIGIN)
        .header("User-Agent", USER_AGENT)
        .header("Cookie", CONSENT_COOKIE)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let json: Value = response.json().await?;
    Ok(parse_search_response(&json))
}

/// Unfiltered failure is an error; songs-filtered failure is ignored. Pure.
fn combine_outcomes<E>(
    unfiltered: Result<Vec<SearchSuggestion>, E>,
    songs: Result<Vec<SearchSuggestion>, E>,
) -> Result<Vec<SearchSuggestion>, E> {
    let unfiltered = unfiltered?;
    Ok(match songs {
        Ok(songs) => merge_results(unfiltered, songs),
        Err(_) => unfiltered,
    })
}

/// Builds the JSON body for a search request. `songs_only == true` adds `"params": SONGS_FILTER_PARAMS`.
/// Pure; must produce valid JSON for any query (quotes, unicode, backslashes) - use serde_json::json!.
pub fn build_request_body(query: &str, songs_only: bool) -> Value {
    let mut body = json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "query": query
    });
    if songs_only {
        body["params"] = Value::String(SONGS_FILTER_PARAMS.to_string());
    }
    body
}

/// Parses an InnerTube search response (unfiltered OR songs-filtered) into suggestions, in on-screen order
/// (top-result card first, then rows in order). Pure, tolerant: never panics on missing/unknown fields;
/// unknown rows are skipped; garbage input returns an empty Vec.
///
/// Layouts to support (see fixtures/):
/// * `contents.tabbedSearchResultsRenderer.tabs[0].tabRenderer.content.sectionListRenderer.contents[]` with
///   - `musicCardShelfRenderer` (top result: title.runs[0] + subtitle.runs, its own `contents[]` rows too),
///   - `itemSectionRenderer.contents[].musicResponsiveListItemRenderer` (unfiltered, flat mixed list),
///   - `musicShelfRenderer.contents[].musicResponsiveListItemRenderer` (songs-filtered / legacy layout),
///   - `messageRenderer` (no results) -> skipped.
/// * Row type is read from the first subtitle run text (English, hl=en): "Song" -> Track; "Album"/"Single"/"EP"
///   -> Album; "Artist" -> Artist. "Video", "Playlist", "Episode", "Podcast", "Profile" and unknown -> skipped.
///   Songs-filtered rows have no type label (subtitle starts with the artist): treat every row there as Track.
///
/// Field mapping (SearchSuggestion, shape frozen):
/// * Track: name=title, band_name=artist runs joined as shown (", "-joined if several), album_name=album run
///   (if present), item_url_path=`https://youtube.com/watch?v={videoId}`, view_count from "857M plays" (parse
///   K/M/B suffix), duration from "m:ss"/"h:mm:ss" in seconds, year None, subscriber_count None, platform Youtube.
/// * Album: name=title, band_name=artist run(s), item_url_path=`https://music.youtube.com/browse/{MPREb_...}`,
///   year from the trailing 4-digit run, album_name None.
/// * Artist: name=title, band_name="Artist", item_url_path=`https://youtube.com/browse/{UC...}`,
///   subscriber_count from "67.4M monthly audience" / "1.7K subscribers" (K/M/B).
/// * img: the thumbnail url; for lh3/yt3.googleusercontent.com urls rewrite `=w60-h60` style size suffix to
///   `=w544-h544` (square, large). Other urls unchanged. Empty string if absent.
pub fn parse_search_response(resp: &Value) -> Vec<SearchSuggestion> {
    let Some(sections) = resp.pointer(SECTIONS_PATH).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for section in sections {
        if let Some(card) = section.get("musicCardShelfRenderer") {
            push_card(card, &mut out);
        } else if let Some(items) = section
            .pointer("/itemSectionRenderer/contents")
            .and_then(Value::as_array)
        {
            push_rows(items, false, &mut out);
        } else if let Some(items) = section
            .pointer("/musicShelfRenderer/contents")
            .and_then(Value::as_array)
        {
            push_rows(items, true, &mut out);
        }
    }
    dedup_by_url(out)
}

/// Merges the unfiltered list with the songs-filtered list. Pure.
/// * Result = all NON-track items of `unfiltered` (order preserved), followed by all tracks:
///   songs-filtered tracks first (they carry album + duration), then leftover unfiltered tracks whose videoId
///   (taken from item_url_path `...watch?v={id}`) does not appear in `songs`.
/// * If `songs` is empty, return `unfiltered` unchanged (original order).
pub fn merge_results(
    unfiltered: Vec<SearchSuggestion>,
    songs: Vec<SearchSuggestion>,
) -> Vec<SearchSuggestion> {
    // Only tracks from the songs list are used; a songs list with no tracks changes nothing.
    let song_tracks: Vec<SearchSuggestion> = songs
        .into_iter()
        .filter(|s| matches!(s.item_type, ItemType::Track))
        .collect();
    if song_tracks.is_empty() {
        return unfiltered;
    }

    let song_ids: HashSet<String> = song_tracks
        .iter()
        .filter_map(|s| video_id_from_url(&s.item_url_path))
        .map(str::to_string)
        .collect();

    let mut merged = Vec::with_capacity(unfiltered.len() + song_tracks.len());
    let mut leftover_tracks = Vec::new();
    for item in unfiltered {
        if !matches!(item.item_type, ItemType::Track) {
            merged.push(item);
        } else if video_id_from_url(&item.item_url_path).is_none_or(|id| !song_ids.contains(id)) {
            leftover_tracks.push(item);
        }
    }
    merged.extend(song_tracks);
    merged.extend(leftover_tracks);
    merged
}

/// Parses "857M plays", "141K plays", "1.2B plays", "67.4M monthly audience", "1.7K subscribers", "38 views",
/// "1,234 views" into a number. Returns None if no leading number. K=1e3, M=1e6, B=1e9. Pure.
pub fn parse_count(text: &str) -> Option<u64> {
    let t = text.trim_start();
    if !t.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let number_end = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(t.len());
    let digits: String = t[..number_end].chars().filter(|&c| c != ',').collect();
    let base: f64 = digits.parse().ok()?;
    let multiplier = match t[number_end..].chars().next() {
        Some('K' | 'k') => 1_000.0,
        Some('M' | 'm') => 1_000_000.0,
        Some('B' | 'b') => 1_000_000_000.0,
        _ => 1.0,
    };
    let value = (base * multiplier).round();
    if value.is_finite() && value >= 0.0 {
        Some(value as u64)
    } else {
        None
    }
}

/// Count of an artist row ("67.4M monthly audience", "1.7K subscribers") plus what it counts. Pure.
pub fn audience_from_segments<'a>(
    texts: impl Iterator<Item = &'a str>,
) -> (Option<u64>, Option<&'static str>) {
    for text in texts {
        if let Some(count) = parse_count(text) {
            let lower = text.to_ascii_lowercase();
            let kind = if lower.contains("monthly") {
                Some("monthly_audience")
            } else if lower.contains("subscriber") {
                Some("subscribers")
            } else {
                None
            };
            return (Some(count), kind);
        }
    }
    (None, None)
}

/// Parses "5:21" -> 321, "1:02:03" -> 3723. None on garbage. Pure.
pub fn parse_duration(text: &str) -> Option<u32> {
    let parts: Vec<u64> = text
        .trim()
        .split(':')
        .map(|part| {
            let part = part.trim();
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        })
        .collect::<Option<Vec<u64>>>()?;

    let total = match parts.as_slice() {
        [minutes, seconds] if *seconds < 60 => minutes.checked_mul(60)?.checked_add(*seconds)?,
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => hours
            .checked_mul(3600)?
            .checked_add(minutes.checked_mul(60)?)?
            .checked_add(*seconds)?,
        _ => return None,
    };
    u32::try_from(total).ok()
}

// ---------------------------------------------------------------------------------------------
// Parsing internals
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Track,
    Album,
    Artist,
}

/// One subtitle segment between " • " separators.
struct Segment {
    text: String,
    /// Texts of runs that carry a navigationEndpoint (linked artist names).
    linked: Vec<String>,
}

/// Everything needed to build one suggestion, extracted from either a card or a list row.
struct Entry<'a> {
    title: String,
    nav: Option<&'a Value>,
    subtitle: &'a [Value],
    plays: String,
    video_hint: Option<&'a str>,
    thumbnail: Option<&'a Value>,
}

fn push_rows(items: &[Value], shelf: bool, out: &mut Vec<SearchSuggestion>) {
    for item in items {
        if let Some(row) = item.get("musicResponsiveListItemRenderer") {
            push_row(row, shelf, out);
        }
    }
}

fn push_card(card: &Value, out: &mut Vec<SearchSuggestion>) {
    let mut card_artist: Option<String> = None;
    if let Some(title_run) = card.pointer("/title/runs/0") {
        let entry = Entry {
            title: text_of(title_run),
            nav: title_run.get("navigationEndpoint"),
            subtitle: runs(card.pointer("/subtitle/runs")),
            plays: String::new(),
            video_hint: None,
            thumbnail: card.get("thumbnail"),
        };
        if let Some(suggestion) = entry_to_suggestion(entry, false) {
            if matches!(suggestion.item_type, ItemType::Artist) {
                card_artist = Some(suggestion.name.clone());
            }
            out.push(suggestion);
        }
    }
    // Rows listed under the card are ordinary rows, emitted after it. Under an Artist card, tracks
    // whose subtitle has no artist (e.g. "Song • 5:38") take the card's artist name as band_name.
    if let Some(contents) = card.get("contents").and_then(Value::as_array) {
        let start = out.len();
        push_rows(contents, false, out);
        if let Some(artist) = card_artist {
            for item in &mut out[start..] {
                if matches!(item.item_type, ItemType::Track) && item.band_name.is_empty() {
                    item.band_name = artist.clone();
                }
            }
        }
    }
}

fn push_row(row: &Value, shelf: bool, out: &mut Vec<SearchSuggestion>) {
    let title_runs = column_runs(row, 0);
    let Some(title_run) = title_runs.first() else {
        return;
    };
    let entry = Entry {
        title: text_of(title_run),
        nav: title_run
            .get("navigationEndpoint")
            .or_else(|| row.get("navigationEndpoint")),
        subtitle: column_runs(row, 1),
        plays: runs_text(column_runs(row, 2)),
        video_hint: row
            .pointer("/playlistItemData/videoId")
            .and_then(Value::as_str),
        thumbnail: row.get("thumbnail"),
    };
    if let Some(suggestion) = entry_to_suggestion(entry, shelf) {
        out.push(suggestion);
    }
}

fn entry_to_suggestion(entry: Entry<'_>, shelf: bool) -> Option<SearchSuggestion> {
    let name = entry.title.trim();
    if name.is_empty() {
        return None;
    }

    let segments = segments(entry.subtitle);
    let (label, rest) = match segments.first() {
        Some(first) if KNOWN_LABELS.contains(&first.text.as_str()) => {
            (Some(first.text.as_str()), &segments[1..])
        }
        _ => (None, &segments[..]),
    };
    let kind = classify(label, shelf)?;
    let img = best_thumbnail(entry.thumbnail);
    let browse_id = entry
        .nav
        .and_then(|nav| nav.pointer("/browseEndpoint/browseId"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());

    match kind {
        Kind::Track => {
            let watch_id = entry
                .nav
                .and_then(|nav| nav.pointer("/watchEndpoint/videoId"))
                .and_then(Value::as_str);
            let video_id = entry
                .video_hint
                .filter(|id| !id.is_empty())
                .or(watch_id.filter(|id| !id.is_empty()))?;

            let non_duration: Vec<&Segment> = rest
                .iter()
                .filter(|s| parse_duration(&s.text).is_none())
                .collect();
            let artist = non_duration
                .first()
                .map(|s| display_artist(s))
                .unwrap_or_default();
            let album = non_duration.get(1).map(|s| s.text.clone());
            let duration = rest.iter().find_map(|s| parse_duration(&s.text));

            Some(SearchSuggestion {
                item_type: ItemType::Track,
                name: name.to_string(),
                band_name: artist,
                album_name: album,
                item_url_path: format!("{WATCH_URL_PREFIX}{video_id}"),
                img,
                subscriber_count: None,
                audience_kind: None,
                view_count: parse_count(&entry.plays),
                year: None,
                duration,
                platform: Platform::Youtube,
            })
        }
        Kind::Album => {
            let browse_id = browse_id?;
            let artist = rest
                .iter()
                .find(|s| parse_year(&s.text).is_none())
                .map(|s| display_artist(s))
                .unwrap_or_default();
            let year = rest.iter().rev().find_map(|s| parse_year(&s.text));

            Some(SearchSuggestion {
                item_type: ItemType::Album,
                name: name.to_string(),
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
        Kind::Artist => {
            let browse_id = browse_id?;
            let (subscriber_count, audience_kind) = audience_from_segments(rest.iter().map(|s| s.text.as_str()));

            Some(SearchSuggestion {
                item_type: ItemType::Artist,
                name: name.to_string(),
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

/// Decides the kind from the type label. Unlabelled rows are tracks only in the songs-filtered shelf.
fn classify(label: Option<&str>, shelf: bool) -> Option<Kind> {
    match label {
        Some("Song") => Some(Kind::Track),
        Some("Album") | Some("Single") | Some("EP") => Some(Kind::Album),
        Some("Artist") => Some(Kind::Artist),
        Some(_) => None,
        None if shelf => Some(Kind::Track),
        None => None,
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

/// Keeps the first item for each item url (card and list may repeat the same video/artist/album).
fn dedup_by_url(items: Vec<SearchSuggestion>) -> Vec<SearchSuggestion> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.item_url_path.clone()))
        .collect()
}

fn video_id_from_url(url: &str) -> Option<&str> {
    url.strip_prefix(WATCH_URL_PREFIX)
        .filter(|id| !id.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    const UNFILTERED_DAFT_PUNK: &str = include_str!("fixtures/ytm_unfiltered_daft_punk.json");
    const UNFILTERED_ONE_MORE_TIME: &str =
        include_str!("fixtures/ytm_unfiltered_one_more_time.json");
    const SONGS_DAFT_PUNK_ONE_MORE_TIME: &str =
        include_str!("fixtures/ytm_songs_filter_daft_punk_one_more_time.json");
    const NO_RESULTS: &str = include_str!("fixtures/ytm_no_results.json");

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is valid JSON")
    }

    fn find_named<'a>(items: &'a [SearchSuggestion], name: &str) -> Option<&'a SearchSuggestion> {
        items.iter().find(|s| s.name == name)
    }

    fn is_track(s: &SearchSuggestion) -> bool {
        matches!(s.item_type, ItemType::Track)
    }

    fn is_album(s: &SearchSuggestion) -> bool {
        matches!(s.item_type, ItemType::Album)
    }

    fn is_artist(s: &SearchSuggestion) -> bool {
        matches!(s.item_type, ItemType::Artist)
    }

    fn sugg(kind: ItemType, name: &str, url: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type: kind,
            name: name.to_string(),
            band_name: String::new(),
            album_name: None,
            item_url_path: url.to_string(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    fn names(items: &[SearchSuggestion]) -> Vec<&str> {
        items.iter().map(|s| s.name.as_str()).collect()
    }

    // ---- synthetic response builders -----------------------------------------------------------

    fn text(t: &str) -> Value {
        json!({ "text": t })
    }

    fn linked(t: &str, nav: Value) -> Value {
        json!({ "text": t, "navigationEndpoint": nav })
    }

    fn bullet() -> Value {
        json!({ "text": " • " })
    }

    fn watch(id: &str) -> Value {
        json!({ "watchEndpoint": { "videoId": id } })
    }

    fn browse(id: &str) -> Value {
        json!({ "browseEndpoint": { "browseId": id } })
    }

    fn thumb(url: &str) -> Value {
        json!({"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
            {"url": url, "width": 60, "height": 60},
            {"url": url, "width": 120, "height": 120}
        ]}}})
    }

    /// A `musicResponsiveListItemRenderer` row wrapper, shaped like the real InnerTube list rows.
    fn list_row(
        title: &str,
        title_nav: Option<Value>,
        subtitle: Vec<Value>,
        plays: Option<&str>,
        hint: Option<&str>,
    ) -> Value {
        let mut title_run = json!({ "text": title });
        if let Some(nav) = title_nav {
            title_run["navigationEndpoint"] = nav;
        }
        let mut columns = vec![
            json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [title_run]}}}),
            json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": subtitle}}}),
        ];
        if let Some(p) = plays {
            columns.push(
                json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": p}]}}}),
            );
        }
        let mut row = json!({
            "flexColumns": columns,
            "thumbnail": thumb("https://lh3.googleusercontent.com/abc=w60-h60-p-l90-rj")
        });
        if let Some(id) = hint {
            row["playlistItemData"] = json!({ "videoId": id });
        }
        json!({ "musicResponsiveListItemRenderer": row })
    }

    fn envelope(sections: Vec<Value>) -> Value {
        json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [
            {"tabRenderer": {"content": {"sectionListRenderer": {"contents": sections}}}}
        ]}}})
    }

    fn shelf(rows: Vec<Value>) -> Value {
        json!({ "musicShelfRenderer": { "contents": rows } })
    }

    /// Unfiltered layout: one `itemSectionRenderer` per row.
    fn mixed(rows: Vec<Value>) -> Vec<Value> {
        rows.into_iter()
            .map(|r| json!({ "itemSectionRenderer": { "contents": [r] } }))
            .collect()
    }

    // ---- constants and request body ------------------------------------------------------------

    #[test]
    fn constants_are_stable() {
        assert_eq!(super::SONGS_FILTER_PARAMS, "EgWKAQIIAWoKEAkQBRAKEAMQBA==");
    }

    #[test]
    fn request_body_has_web_remix_context_and_query() {
        let body = build_request_body("daft punk", false);
        assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
        assert_eq!(body["context"]["client"]["clientVersion"], CLIENT_VERSION);
        assert_eq!(body["context"]["client"]["hl"], "en");
        assert_eq!(body["context"]["client"]["gl"], "US");
        assert_eq!(body["query"], "daft punk");
    }

    #[test]
    fn request_body_unfiltered_has_no_params() {
        let body = build_request_body("x", false);
        assert!(body.get("params").is_none());
    }

    #[test]
    fn request_body_songs_only_adds_filter_params() {
        let body = build_request_body("x", true);
        assert_eq!(body["params"], SONGS_FILTER_PARAMS);
    }

    #[test]
    fn request_body_escapes_quotes_backslashes_and_unicode() {
        let query = "Beyoncé \"quoted\" \\ 日本語 🎵 \n tab\t";
        let body = build_request_body(query, true);
        assert_eq!(body["query"], query);
        let serialized = serde_json::to_string(&body).expect("serializes");
        let back: Value = serde_json::from_str(&serialized).expect("valid JSON");
        assert_eq!(back, body);
    }

    #[test]
    fn request_body_empty_query_is_still_valid() {
        let body = build_request_body("", false);
        assert_eq!(body["query"], "");
    }

    // ---- counts and durations ------------------------------------------------------------------

    #[test]
    fn parse_count_table() {
        let cases: [(&str, Option<u64>); 14] = [
            ("857M plays", Some(857_000_000)),
            ("141K plays", Some(141_000)),
            ("1.2B plays", Some(1_200_000_000)),
            ("67.4M monthly audience", Some(67_400_000)),
            ("1.7K subscribers", Some(1_700)),
            ("1.77K subscribers", Some(1_770)),
            ("1.7k subscribers", Some(1_700)),
            ("38 views", Some(38)),
            ("1,234 views", Some(1_234)),
            ("1,234K plays", Some(1_234_000)),
            ("0 plays", Some(0)),
            ("  12 plays", Some(12)),
            ("12 Bands", Some(12)),
            ("1,2,3", Some(123)),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_count(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_count_garbage_is_none() {
        for input in [
            "",
            "   ",
            "plays",
            "K plays",
            "M",
            ".5M",
            "1.2.3M",
            "-5",
            "Plays: 12",
            "日本 plays",
            "🎵 12",
            "５ plays", // fullwidth digit is not an ASCII digit
        ] {
            assert_eq!(parse_count(input), None, "input {input:?}");
        }
    }

    #[test]
    fn parse_duration_table() {
        let cases: [(&str, Option<u32>); 9] = [
            ("5:21", Some(321)),
            ("1:02:03", Some(3723)),
            ("0:00", Some(0)),
            ("12:00", Some(720)),
            (" 3:05 ", Some(185)),
            ("1:2", Some(62)),
            ("1:02:03:04", None),
            ("1:60", None),
            ("1:02:60", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_duration(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn parse_duration_garbage_is_none_without_panic() {
        for input in [
            "",
            "5",
            "5:",
            ":21",
            "a:b",
            "-1:00",
            "1:2:3:4",
            "５:21",
            "99999999999999999999:00",
            "1:-2",
        ] {
            assert_eq!(parse_duration(input), None, "input {input:?}");
        }
    }

    // ---- image upscaling -----------------------------------------------------------------------

    #[test]
    fn upscale_img_rewrites_google_size_suffix() {
        assert_eq!(
            upscale_img("https://lh3.googleusercontent.com/x=w60-h60-p-l90-rj"),
            "https://lh3.googleusercontent.com/x=w544-h544-p-l90-rj"
        );
        assert_eq!(
            upscale_img("https://yt3.googleusercontent.com/y=w120-h120-l90-rj"),
            "https://yt3.googleusercontent.com/y=w544-h544-l90-rj"
        );
    }

    #[test]
    fn upscale_img_leaves_other_urls_unchanged() {
        for url in [
            "https://example.com/a=w60-h60",
            "https://lh3.googleusercontent.com/z=s60",
            "https://lh3.googleusercontent.com/z=w60",
            "https://lh3.googleusercontent.com/z",
            "",
        ] {
            assert_eq!(upscale_img(url), url);
        }
    }

    // ---- fixtures ------------------------------------------------------------------------------

    #[test]
    fn unfiltered_daft_punk_top_card_is_artist_with_subscribers() {
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        let first = &items[0];
        assert!(is_artist(first));
        assert_eq!(first.name, "Daft Punk");
        assert_eq!(first.band_name, "Artist");
        assert_eq!(first.subscriber_count, Some(67_400_000));
        assert_eq!(
            first.item_url_path,
            "https://youtube.com/browse/UCRr1xG_2WIDs18a6cIiCxeA"
        );
        assert!(first.img.contains("=w544-h544"), "img was {}", first.img);
    }

    #[test]
    fn unfiltered_daft_punk_card_rows_follow_the_card() {
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        assert!(is_track(&items[1]));
        assert!(items[1].name.starts_with("Instant Crush"));
        assert_eq!(items[1].view_count, Some(1_200_000_000));
    }

    #[test]
    fn card_rows_under_artist_card_take_the_card_artist() {
        // "Instant Crush" has no artist in its subtitle ("Song • 5:38"); the Daft Punk artist card fills it in.
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        let crush = find_named(&items, "Instant Crush (feat. Julian Casablancas)").expect("row present");
        assert_eq!(crush.band_name, "Daft Punk");
        assert_eq!(crush.duration, Some(338));
    }

    #[test]
    fn card_rows_under_song_card_keep_empty_artist() {
        // Only Artist cards lend their name; a Song card's rows are left as parsed.
        let card = json!({
            "musicCardShelfRenderer": {
                "title": {"runs": [{"text": "Top Song", "navigationEndpoint": watch("vidT")}]},
                "subtitle": {"runs": [text("Song"), bullet(), text("Someone"), bullet(), text("3:00")]},
                "contents": [list_row("Row", None, vec![text("Song"), bullet(), text("3:10")], None, Some("vidR"))]
            }
        });
        let items = parse_search_response(&envelope(vec![card]));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].band_name, "Someone");
        assert_eq!(items[1].name, "Row");
        assert_eq!(items[1].band_name, "");
    }

    #[test]
    fn unfiltered_daft_punk_has_random_access_memories_album() {
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        let ram = find_named(&items, "Random Access Memories").expect("album present");
        assert!(is_album(ram));
        assert_eq!(ram.year, Some(2013));
        assert_eq!(ram.band_name, "Daft Punk");
        assert!(ram
            .item_url_path
            .starts_with("https://music.youtube.com/browse/MPREb_"));
        assert!(items.iter().any(is_track));
    }

    #[test]
    fn unfiltered_daft_punk_skips_videos_and_playlists() {
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        // Video ids from "Video" rows must not leak through as tracks.
        for video_id in ["PwILkY9gRrc", "dwDns8x3Jb4", "D8K90hX4PrE"] {
            assert!(
                items.iter().all(|s| !s.item_url_path.contains(video_id)),
                "video {video_id} leaked"
            );
        }
        // The "Playlist" row titled "Daft Punk" is skipped, so only the card artist remains.
        let daft_punk_artists = items
            .iter()
            .filter(|s| is_artist(s) && s.name == "Daft Punk")
            .count();
        assert_eq!(daft_punk_artists, 1);
        assert!(items.iter().all(|s| s.band_name != "Jonathan Titou"));
    }

    #[test]
    fn unfiltered_daft_punk_urls_follow_conventions_and_are_unique() {
        let items = parse_search_response(&fixture(UNFILTERED_DAFT_PUNK));
        assert!(!items.is_empty());
        let mut urls = HashSet::new();
        for item in &items {
            assert!(
                item.item_url_path.starts_with(WATCH_URL_PREFIX)
                    || item.item_url_path.starts_with(ARTIST_URL_PREFIX)
                    || item.item_url_path.starts_with(ALBUM_URL_PREFIX),
                "bad url {}",
                item.item_url_path
            );
            assert!(!matches!(item.item_type, ItemType::Unknown));
            assert!(urls.insert(item.item_url_path.clone()), "duplicate url");
        }
    }

    #[test]
    fn unfiltered_one_more_time_top_card_song_is_track_first() {
        // The captured top card is a Song ("...Baby One More Time"), not a Video, so it is emitted first.
        let items = parse_search_response(&fixture(UNFILTERED_ONE_MORE_TIME));
        let first = &items[0];
        assert!(is_track(first));
        assert_eq!(first.name, "...Baby One More Time");
        assert_eq!(first.band_name, "Britney Spears");
        assert_eq!(first.duration, Some(212));
        assert_eq!(first.item_url_path, "https://youtube.com/watch?v=91Niv2q4gvc");
    }

    #[test]
    fn unfiltered_one_more_time_radio_edit_and_single() {
        let items = parse_search_response(&fixture(UNFILTERED_ONE_MORE_TIME));
        let radio = find_named(&items, "One More Time (Radio Edit)").expect("radio edit present");
        assert!(is_track(radio));
        assert_eq!(radio.band_name, "Daft Punk");
        assert_eq!(radio.view_count, Some(857_000_000));
        assert_eq!(radio.item_url_path, "https://youtube.com/watch?v=wU26xVT_vBU");

        let single = items
            .iter()
            .find(|s| is_album(s) && s.name == "One More Time" && s.band_name == "Daft Punk")
            .expect("single present");
        assert_eq!(single.year, Some(2000));
    }

    #[test]
    fn unfiltered_one_more_time_episode_is_skipped() {
        let items = parse_search_response(&fixture(UNFILTERED_ONE_MORE_TIME));
        assert!(items.iter().all(|s| !s.name.starts_with("Bianca Balti")));
    }

    #[test]
    fn top_card_labelled_video_is_skipped() {
        let mut resp = fixture(UNFILTERED_ONE_MORE_TIME);
        let label = resp
            .pointer_mut(
                "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicCardShelfRenderer/subtitle/runs/0/text",
            )
            .expect("card label present in fixture");
        *label = json!("Video");

        let items = parse_search_response(&resp);
        assert!(items.iter().all(|s| s.name != "...Baby One More Time"));
        assert!(is_track(&items[0]));
        assert_eq!(items[0].name, "One More Time (Radio Edit)");
    }

    #[test]
    fn songs_filter_all_rows_are_tracks_with_album_and_duration() {
        let items = parse_search_response(&fixture(SONGS_DAFT_PUNK_ONE_MORE_TIME));
        assert_eq!(items.len(), 20);
        assert!(items.iter().all(is_track));
        assert!(items.iter().all(|s| s.duration.is_some()));
        assert!(items.iter().all(|s| s.album_name.is_some()));

        let first = &items[0];
        assert_eq!(first.name, "One More Time (Radio Edit)");
        assert_eq!(first.album_name.as_deref(), Some("One More Time"));
        assert_eq!(first.duration, Some(321));
        assert_eq!(first.band_name, "Daft Punk");
        assert_eq!(first.view_count, Some(857_000_000));
    }

    #[test]
    fn no_results_fixture_is_empty() {
        assert!(parse_search_response(&fixture(NO_RESULTS)).is_empty());
    }

    #[test]
    fn garbage_top_level_values_yield_empty_without_panic() {
        let values = [
            Value::Null,
            json!({}),
            json!([1, 2, 3]),
            json!("text"),
            json!(42),
            json!({"contents": 5}),
            json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": []}}}),
            json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {}}]}}}),
        ];
        for value in &values {
            assert!(parse_search_response(value).is_empty(), "value {value}");
        }
    }

    #[test]
    fn malformed_rows_are_skipped_without_panic() {
        let rows = vec![
            json!({"musicResponsiveListItemRenderer": {}}),
            json!({"musicResponsiveListItemRenderer": {"flexColumns": null}}),
            json!({"musicResponsiveListItemRenderer": {"flexColumns": []}}),
            json!({"musicResponsiveListItemRenderer": {"flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": []}}}
            ]}}),
            json!({"musicResponsiveListItemRenderer": {"flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "   "}]}}}
            ]}}),
            json!("not a row"),
            json!({"unrelated": true}),
        ];
        let resp = envelope(vec![shelf(rows.clone()), json!({"itemSectionRenderer": {"contents": rows}})]);
        assert!(parse_search_response(&resp).is_empty());
    }

    #[test]
    fn card_with_garbage_is_skipped() {
        let resp = envelope(vec![json!({"musicCardShelfRenderer": "oops"})]);
        assert!(parse_search_response(&resp).is_empty());
    }

    // ---- synthetic rows ------------------------------------------------------------------------

    #[test]
    fn mixed_labelled_rows_map_to_kinds_and_skip_others() {
        let rows = vec![
            list_row(
                "Song A",
                Some(watch("vidA")),
                vec![text("Song"), bullet(), linked("Artist X", browse("UCx"))],
                Some("12K plays"),
                None,
            ),
            list_row(
                "Vid",
                Some(watch("vidV")),
                vec![text("Video"), bullet(), text("Artist X"), bullet(), text("5M views")],
                None,
                None,
            ),
            list_row(
                "Album B",
                Some(browse("MPREb_bbb")),
                vec![text("Album"), bullet(), text("Artist X"), bullet(), text("2011")],
                None,
                None,
            ),
            list_row(
                "Artist Y",
                Some(browse("UCy")),
                vec![text("Artist"), bullet(), text("1.7K subscribers")],
                None,
                None,
            ),
            list_row(
                "PL",
                Some(browse("VLPL")),
                vec![text("Playlist"), bullet(), text("Someone")],
                None,
                None,
            ),
        ];
        let items = parse_search_response(&envelope(mixed(rows)));
        assert_eq!(names(&items), ["Song A", "Album B", "Artist Y"]);

        let track = &items[0];
        assert!(is_track(track));
        assert_eq!(track.band_name, "Artist X");
        assert_eq!(track.view_count, Some(12_000));
        assert_eq!(track.album_name, None);
        assert_eq!(track.item_url_path, "https://youtube.com/watch?v=vidA");

        let album = &items[1];
        assert!(is_album(album));
        assert_eq!(album.year, Some(2011));
        assert_eq!(album.band_name, "Artist X");
        assert_eq!(album.item_url_path, "https://music.youtube.com/browse/MPREb_bbb");

        let artist = &items[2];
        assert!(is_artist(artist));
        assert_eq!(artist.subscriber_count, Some(1_700));
        assert_eq!(artist.item_url_path, "https://youtube.com/browse/UCy");
    }

    #[test]
    fn mixed_unlabelled_rows_are_skipped() {
        // Only the songs-filtered shelf treats unlabelled rows as tracks.
        let rows = vec![list_row(
            "Mystery",
            Some(watch("vidM")),
            vec![text("Band"), bullet(), text("3:00")],
            None,
            None,
        )];
        assert!(parse_search_response(&envelope(mixed(rows))).is_empty());
    }

    #[test]
    fn shelf_unlabelled_row_is_track_and_labelled_video_is_skipped() {
        let rows = vec![
            list_row(
                "Plain",
                None,
                vec![
                    linked("Band", browse("UCb")),
                    bullet(),
                    text("Album Z"),
                    bullet(),
                    text("3:21"),
                ],
                Some("1M plays"),
                Some("vid1"),
            ),
            list_row(
                "Labelled video",
                Some(watch("vidV")),
                vec![text("Video"), bullet(), text("Band")],
                None,
                None,
            ),
        ];
        let items = parse_search_response(&envelope(vec![shelf(rows)]));
        assert_eq!(items.len(), 1);
        let track = &items[0];
        assert!(is_track(track));
        assert_eq!(track.name, "Plain");
        assert_eq!(track.band_name, "Band");
        assert_eq!(track.album_name.as_deref(), Some("Album Z"));
        assert_eq!(track.duration, Some(201));
        assert_eq!(track.view_count, Some(1_000_000));
        assert_eq!(track.item_url_path, "https://youtube.com/watch?v=vid1");
        assert_eq!(track.img, "https://lh3.googleusercontent.com/abc=w544-h544-p-l90-rj");
    }

    #[test]
    fn multiple_linked_artists_are_joined_with_comma() {
        let row = list_row(
            "Duet",
            None,
            vec![
                linked("A", browse("UC1")),
                text(" & "),
                linked("B", browse("UC2")),
                bullet(),
                text("Album"),
                bullet(),
                text("3:21"),
            ],
            None,
            Some("vidD"),
        );
        let items = parse_search_response(&envelope(vec![shelf(vec![row])]));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].band_name, "A, B");
        assert_eq!(items[0].album_name.as_deref(), Some("Album"));
        assert_eq!(items[0].duration, Some(201));
    }

    #[test]
    fn track_without_any_video_id_is_skipped() {
        let row = list_row("No id", None, vec![text("Band")], None, None);
        assert!(parse_search_response(&envelope(vec![shelf(vec![row])])).is_empty());
    }

    #[test]
    fn duplicate_video_keeps_first_occurrence() {
        let rows = vec![
            list_row("First", None, vec![text("Band")], None, Some("vid1")),
            list_row("Second", None, vec![text("Band")], None, Some("vid1")),
        ];
        let items = parse_search_response(&envelope(vec![shelf(rows)]));
        assert_eq!(names(&items), ["First"]);
    }

    #[test]
    fn missing_plays_gives_no_view_count() {
        let row = list_row("Quiet", None, vec![text("Band")], Some(""), Some("vidQ"));
        let items = parse_search_response(&envelope(vec![shelf(vec![row])]));
        assert_eq!(items[0].view_count, None);
    }

    // ---- merge ---------------------------------------------------------------------------------

    #[test]
    fn merge_with_empty_songs_returns_unfiltered_unchanged() {
        let unfiltered = vec![
            sugg(ItemType::Artist, "Ar", "https://youtube.com/browse/UC1"),
            sugg(ItemType::Track, "t1", "https://youtube.com/watch?v=vid1"),
            sugg(ItemType::Album, "Al", "https://music.youtube.com/browse/MPREb_1"),
            sugg(ItemType::Track, "t2", "https://youtube.com/watch?v=vid2"),
        ];
        let merged = merge_results(unfiltered, Vec::new());
        assert_eq!(names(&merged), ["Ar", "t1", "Al", "t2"]);
    }

    #[test]
    fn merge_puts_non_tracks_first_then_song_tracks_then_leftovers() {
        let unfiltered = vec![
            sugg(ItemType::Artist, "Ar", "https://youtube.com/browse/UC1"),
            sugg(ItemType::Track, "t1", "https://youtube.com/watch?v=vid1"),
            sugg(ItemType::Album, "Al", "https://music.youtube.com/browse/MPREb_1"),
            sugg(ItemType::Track, "t2", "https://youtube.com/watch?v=vid2"),
            sugg(ItemType::Track, "t3", "https://youtube.com/watch?v=vid3"),
        ];
        let songs = vec![
            sugg(ItemType::Track, "s2", "https://youtube.com/watch?v=vid2"),
            sugg(ItemType::Track, "s9", "https://youtube.com/watch?v=vid9"),
        ];
        let merged = merge_results(unfiltered, songs);
        assert_eq!(names(&merged), ["Ar", "Al", "s2", "s9", "t1", "t3"]);
    }

    #[test]
    fn merge_ignores_non_track_songs() {
        let unfiltered = vec![
            sugg(ItemType::Track, "t1", "https://youtube.com/watch?v=vid1"),
            sugg(ItemType::Artist, "Ar", "https://youtube.com/browse/UC1"),
        ];
        // Songs list with no tracks at all changes nothing.
        let only_album = vec![sugg(ItemType::Album, "x", "https://music.youtube.com/browse/MPREb_x")];
        assert_eq!(names(&merge_results(unfiltered.clone(), only_album)), ["t1", "Ar"]);

        // Non-track entries in a songs list that does have tracks are dropped.
        let mixed_songs = vec![
            sugg(ItemType::Album, "x", "https://music.youtube.com/browse/MPREb_x"),
            sugg(ItemType::Track, "s1", "https://youtube.com/watch?v=vid5"),
        ];
        assert_eq!(names(&merge_results(unfiltered, mixed_songs)), ["Ar", "s1", "t1"]);
    }

    #[test]
    fn merge_keeps_track_with_unparseable_url_as_leftover() {
        let unfiltered = vec![sugg(ItemType::Track, "odd", "https://example.com/x")];
        let songs = vec![sugg(ItemType::Track, "s1", "https://youtube.com/watch?v=vid5")];
        assert_eq!(names(&merge_results(unfiltered, songs)), ["s1", "odd"]);
    }

    #[test]
    fn merge_of_two_empty_lists_is_empty() {
        assert!(merge_results(Vec::new(), Vec::new()).is_empty());
    }

    // ---- outcome combination (no network) ------------------------------------------------------

    #[test]
    fn unfiltered_error_is_an_error_even_if_songs_succeed() {
        let songs: Result<Vec<SearchSuggestion>, String> =
            Ok(vec![sugg(ItemType::Track, "s", "https://youtube.com/watch?v=v")]);
        let result = combine_outcomes(Err("boom".to_string()), songs);
        assert_eq!(result.unwrap_err(), "boom");
    }

    #[test]
    fn songs_error_is_ignored() {
        let unfiltered: Result<Vec<SearchSuggestion>, String> =
            Ok(vec![sugg(ItemType::Artist, "Ar", "https://youtube.com/browse/UC1")]);
        let songs: Result<Vec<SearchSuggestion>, String> = Err("timeout".to_string());
        let merged = combine_outcomes(unfiltered, songs).expect("unfiltered succeeded");
        assert_eq!(names(&merged), ["Ar"]);
    }

    #[test]
    fn both_ok_are_merged() {
        let unfiltered: Result<Vec<SearchSuggestion>, String> = Ok(vec![
            sugg(ItemType::Track, "t1", "https://youtube.com/watch?v=vid1"),
            sugg(ItemType::Album, "Al", "https://music.youtube.com/browse/MPREb_1"),
        ]);
        let songs: Result<Vec<SearchSuggestion>, String> =
            Ok(vec![sugg(ItemType::Track, "s1", "https://youtube.com/watch?v=vid1")]);
        let merged = combine_outcomes(unfiltered, songs).expect("ok");
        assert_eq!(names(&merged), ["Al", "s1"]);
    }

    #[test]
    fn audience_kind_is_taken_from_the_label() {
        assert_eq!(
            audience_from_segments(["Artist", "67.4M monthly audience"].into_iter()),
            (Some(67_400_000), Some("monthly_audience"))
        );
        assert_eq!(
            audience_from_segments(["1.7K subscribers"].into_iter()),
            (Some(1_700), Some("subscribers"))
        );
        assert_eq!(audience_from_segments(["5K"].into_iter()), (Some(5_000), None));
        assert_eq!(audience_from_segments(["Artist"].into_iter()), (None, None));
    }
}
