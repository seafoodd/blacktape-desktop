//! Bandcamp side of the full search page (search v2): filtered search + artist page scrape.
//!
//! OWNER: part B. Contract = v2\DECISIONS.md D9, D11, D12 (VERIFIED LIVE 2026-11-09).
//! * Search: `POST https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic`
//!   body `{"search_text": q, "search_filter": "a"|"t"|"b", "full_page": true}`. Returns up to 50 albums /
//!   50 tracks / 10 artists. THERE IS NO PAGING (any extra field -> `UnexpectedParamError`) and the HTML
//!   search page sits behind a bot challenge, so Bandcamp contributes only to the FIRST page.
//! * Artist url: result type "b" has NO `item_url_path`, only `item_url_root` ("https://x.bandcamp.com")
//!   -> use `item_url_root` as `SearchSuggestion::item_url_path` for artists (the v1 `bandcamp.rs`
//!   converter leaves it empty; do not edit bandcamp.rs, write your own parsing here).
//! * Image rewrite like `bandcamp.rs`: for albums/tracks `img.replace("/img/", "/img/a")`, artists unchanged.
//! * Artist page: `GET {artist root}/music` (plain HTML, no challenge, ~0.6 s), parse with regex (no new deps).
//!
//! Fixtures: `fixtures/bc_search_filter_{a,t,b}_daft_punk.json`, `fixtures/bc_artist_page_shitty_daft_punk.html`.

#![allow(dead_code)]

use crate::search::full::{ArtistProfile, SearchKind};
use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::error::Error;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

const SEARCH_URL: &str = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";
/// Timeout of one filtered search request (D18).
const SEARCH_TIMEOUT: Duration = Duration::from_secs(4);
/// Timeout of the artist page request.
const PROFILE_TIMEOUT: Duration = Duration::from_secs(5);

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Lazily compiled static regex.
macro_rules! regex {
    ($pattern:expr) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($pattern).expect("static regex is valid"))
    }};
}

/// `search_filter` letter per kind: Track "t", Album "a", Artist "b".
pub fn filter_for(kind: SearchKind) -> &'static str {
    match kind {
        SearchKind::Track => "t",
        SearchKind::Album => "a",
        SearchKind::Artist => "b",
    }
}

/// JSON request body (D9). Must be valid for any query (quotes, unicode). Pure.
pub fn build_request_body(query: &str, kind: SearchKind) -> Value {
    json!({
        "search_text": query,
        "search_filter": filter_for(kind),
        "full_page": true,
    })
}

/// Parses the API response text into suggestions of `kind` (only results whose `type` matches the kind; garbage,
/// `UnexpectedParamError` bodies and empty results -> empty Vec, never panics). Field mapping like `bandcamp.rs`
/// (`platform = Bandcamp`, year/duration/counts None) except the artist url rule above. In-page order is kept.
/// Rows without a name or without a url (album/track `item_url_path`, artist `item_url_root`) are skipped:
/// they cannot be downloaded and would collide in deduplication. Deduplicated by url.
pub fn parse_results(body: &str, kind: SearchKind) -> Vec<SearchSuggestion> {
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(results) = json.pointer("/auto/results").and_then(Value::as_array) else {
        return Vec::new();
    };
    let wanted = filter_for(kind);
    let mut seen = HashSet::new();
    results
        .iter()
        .filter(|result| result.get("type").and_then(Value::as_str) == Some(wanted))
        .filter_map(|result| parse_result(result, kind))
        .filter(|item| seen.insert(item.item_url_path.clone()))
        .collect()
}

/// One result object -> suggestion, None when a required field is missing.
fn parse_result(result: &Value, kind: SearchKind) -> Option<SearchSuggestion> {
    let name = str_field(result, "name")?.trim();
    if name.is_empty() {
        return None;
    }
    let url = match kind {
        SearchKind::Artist => str_field(result, "item_url_root"),
        SearchKind::Album | SearchKind::Track => str_field(result, "item_url_path"),
    }?;
    let img = str_field(result, "img").unwrap_or_default();
    let img = match kind {
        SearchKind::Artist => img.to_string(),
        SearchKind::Album | SearchKind::Track => img.replace("/img/", "/img/a"),
    };
    let item_type = match kind {
        SearchKind::Track => ItemType::Track,
        SearchKind::Album => ItemType::Album,
        SearchKind::Artist => ItemType::Artist,
    };
    Some(SearchSuggestion {
        item_type,
        name: name.to_string(),
        band_name: str_field(result, "band_name").unwrap_or(name).to_string(),
        album_name: str_field(result, "album_name").map(str::to_string),
        item_url_path: url.to_string(),
        img,
        subscriber_count: None,
        audience_kind: None,
        view_count: None,
        year: None,
        duration: None,
        platform: Platform::Bandcamp,
    })
}

/// Non-empty string field of a JSON object.
fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

fn http_client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .pool_idle_timeout(Duration::from_secs(60))
            // Redirects may only stay on Bandcamp hosts, at most 3 hops.
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3 {
                    attempt.error("too many redirects")
                } else if attempt.url().host_str().is_some_and(is_bandcamp_host) {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// One filtered request (timeout 4 s; own static reqwest client). Err on network/non-2xx.
pub async fn search_page(
    query: &str,
    kind: SearchKind,
) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let response = http_client()
        .post(SEARCH_URL)
        .json(&build_request_body(query, kind))
        .timeout(SEARCH_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    let body = response.text().await?;
    Ok(parse_results(&body, kind))
}

/// `bandcamp.com` itself or any `*.bandcamp.com` subdomain. Lookalikes (`bandcamp.com.evil.com`,
/// `evilbandcamp.com`) and IP/localhost hosts are rejected. Pure.
fn is_bandcamp_host(host: &str) -> bool {
    host == "bandcamp.com" || host.ends_with(".bandcamp.com")
}

/// `https://x.bandcamp.com/...` -> `https://x.bandcamp.com` (scheme + host only). Only https and Bandcamp hosts
/// (`bandcamp.com` / `*.bandcamp.com`) are accepted: the url comes from the frontend, so other hosts (including
/// Bandcamp custom domains, which then get no profile card) are refused. None otherwise. Pure.
pub fn site_root(url: &str) -> Option<String> {
    let parsed = Url::parse(url.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str().filter(|host| is_bandcamp_host(host))?;
    Some(format!("https://{host}"))
}

/// Fetches `{root}/music` and parses it (timeout 5 s). `url` must be an https `*.bandcamp.com` (or `bandcamp.com`) root or
/// any page on it; only scheme+host are used. Other schemes/empty -> Err. Err also when the page has no name
/// (for example a bot-challenge page).
pub async fn artist_profile(url: &str) -> Result<ArtistProfile, Box<dyn Error + Send + Sync>> {
    let root = site_root(url).ok_or("not a Bandcamp artist url")?;
    let response = http_client()
        .get(format!("{root}/music"))
        .timeout(PROFILE_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    let html = response.text().await?;
    let profile = parse_artist_page(&html, &root);
    if profile.name.is_empty() {
        return Err("Bandcamp artist page had no name".into());
    }
    Ok(profile)
}

/// Max tracks shown as "top songs" of a Bandcamp profile.
const TOP_SONGS_LIMIT: usize = 5;

/// Parses the artist page HTML (D11): name from `<span class="title">` in `#band-name-location` (falls back to
/// `og:title`), location from `span.location`, avatar from `og:image`, bio best-effort (`<p>` inside `#bio-container`),
/// discography from `<li ... data-item-id="album-123|track-123">` entries (`<a href>` relative -> make absolute with `base_url`,
/// `<img src>`, `<p class="title">` trimmed, HTML-unescaped via the `html-escape` crate). `album-*` -> `albums` (ItemType::Album),
/// `track-*` -> the first 5 become `top_songs`, the rest `singles` (ItemType::Track). Tolerant: missing parts -> None/empty.
pub fn parse_artist_page(html: &str, base_url: &str) -> ArtistProfile {
    let name = regex!(r#"(?s)<p id="band-name-location">\s*<span class="title">(.*?)</span>"#)
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| clean_text(m.as_str()))
        .filter(|name| !name.is_empty())
        .or_else(|| meta_content(html, "og:title"))
        .unwrap_or_default();
    let location = regex!(r#"(?s)<span class="location[^"]*">(.*?)</span>"#)
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| clean_text(m.as_str()))
        .filter(|location| !location.is_empty());
    let avatar = meta_content(html, "og:image").filter(|url| !url.is_empty());
    let description = bio_text(html);
    let base = Url::parse(base_url).ok();

    let mut albums = Vec::new();
    let mut singles = Vec::new();
    let mut seen = HashSet::new();
    for capture in regex!(r#"(?s)<li\s[^>]*data-item-id="((?:album|track)-\d+)"[^>]*>(.*?)</li>"#)
        .captures_iter(html)
    {
        let (Some(id), Some(body)) = (capture.get(1), capture.get(2)) else {
            continue;
        };
        let is_album = id.as_str().starts_with("album-");
        let href = regex!(r#"<a\s[^>]*href="([^"]*)""#)
            .captures(body.as_str())
            .and_then(|c| c.get(1))
            .map(|m| decode(m.as_str()));
        let Some(url) = href.and_then(|href| resolve(base.as_ref(), base_url, &href)) else {
            continue;
        };
        let Some(title) = regex!(r#"(?s)<p class="title">(.*?)</p>"#)
            .captures(body.as_str())
            .and_then(|c| c.get(1))
            .map(|m| clean_text(m.as_str()))
            .filter(|title| !title.is_empty())
        else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let img = regex!(r#"<img[^>]*src="([^"]*)""#)
            .captures(body.as_str())
            .and_then(|c| c.get(1))
            .map(|m| decode(m.as_str()))
            .unwrap_or_default();
        let item = SearchSuggestion {
            item_type: if is_album { ItemType::Album } else { ItemType::Track },
            name: title,
            band_name: name.clone(),
            album_name: None,
            item_url_path: url,
            img,
            subscriber_count: None,
            audience_kind: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Bandcamp,
        };
        if is_album {
            albums.push(item);
        } else {
            singles.push(item);
        }
    }

    // Like YouTube's "Top songs": the first standalone tracks; the rest stay in `singles`.
    let top_songs: Vec<SearchSuggestion> = singles.drain(..singles.len().min(TOP_SONGS_LIMIT)).collect();

    ArtistProfile {
        platform: Some(Platform::Bandcamp),
        name,
        url: base_url.to_string(),
        avatar,
        banner: None,
        subscribers: None,
        monthly_audience: None,
        description,
        location,
        top_songs,
        albums,
        singles,
    }
}

/// Bio paragraphs inside `#bio-container` (up to `#music-grid`), joined with blank lines. None when there are none.
fn bio_text(html: &str) -> Option<String> {
    let start = html.find(r#"id="bio-container""#)?;
    let rest = &html[start..];
    let end = rest.find(r#"id="music-grid""#).unwrap_or(rest.len());
    let paragraphs: Vec<String> = regex!(r"(?s)<p[^>]*>(.*?)</p>")
        .captures_iter(&rest[..end])
        .filter_map(|c| c.get(1))
        .map(|m| clean_text(m.as_str()))
        .filter(|text| !text.is_empty())
        .collect();
    (!paragraphs.is_empty()).then(|| paragraphs.join("\n\n"))
}

/// `content` of the first `<meta>` tag with `property="{property}"`, unescaped. None when absent or empty.
fn meta_content(html: &str, property: &str) -> Option<String> {
    let wanted = format!(r#"property="{property}""#);
    regex!(r"(?i)<meta\s[^>]*>")
        .find_iter(html)
        .map(|m| m.as_str())
        .find(|tag| tag.contains(&wanted))
        .and_then(|tag| regex!(r#"content="([^"]*)""#).captures(tag))
        .and_then(|c| c.get(1))
        .map(|m| decode(m.as_str()))
        .filter(|content| !content.is_empty())
}

/// Resolves an href against the page base (absolute hrefs are kept, relative ones made absolute). None if unparsable.
fn resolve(base: Option<&Url>, base_url: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if let Ok(absolute) = Url::parse(href) {
        return Some(absolute.to_string());
    }
    let base = match base {
        Some(base) => base.clone(),
        None => Url::parse(base_url).ok()?,
    };
    base.join(href).ok().map(|url| url.to_string())
}

/// HTML-unescapes an attribute value (no whitespace collapsing, no tag stripping).
fn decode(text: &str) -> String {
    html_escape::decode_html_entities(text.trim()).into_owned()
}

/// Strips tags, unescapes entities and collapses whitespace runs into single spaces.
fn clean_text(html: &str) -> String {
    let without_tags = regex!(r"<[^>]*>").replace_all(html, " ");
    let stripped: &str = &without_tags;
    let decoded = html_escape::decode_html_entities(stripped);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULTS_ALBUMS: &str = include_str!("fixtures/bc_search_filter_a_daft_punk.json");
    const RESULTS_TRACKS: &str = include_str!("fixtures/bc_search_filter_t_daft_punk.json");
    const RESULTS_ARTISTS: &str = include_str!("fixtures/bc_search_filter_b_daft_punk.json");
    const ARTIST_PAGE: &str = include_str!("fixtures/bc_artist_page_shitty_daft_punk.html");
    const UNEXPECTED_PARAM: &str = r#"{"__api_special__":"exception","error_type":"Endpoints::UnexpectedParamError"}"#;

    const ALL_KINDS: [SearchKind; 3] = [SearchKind::Track, SearchKind::Album, SearchKind::Artist];

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    #[test]
    fn filters_per_kind() {
        assert_eq!(filter_for(SearchKind::Track), "t");
        assert_eq!(filter_for(SearchKind::Album), "a");
        assert_eq!(filter_for(SearchKind::Artist), "b");
    }

    #[test]
    fn fixtures_are_valid() {
        for f in [RESULTS_ALBUMS, RESULTS_TRACKS, RESULTS_ARTISTS] {
            serde_json::from_str::<Value>(f).unwrap();
        }
        assert!(ARTIST_PAGE.contains("music-grid"));
    }

    // ----- request body -----

    #[test]
    fn request_body_shape_per_kind() {
        for kind in ALL_KINDS {
            let body = build_request_body("daft punk", kind);
            assert_eq!(body["search_text"], "daft punk");
            assert_eq!(body["search_filter"], filter_for(kind));
            assert_eq!(body["full_page"], true);
            assert_eq!(body.as_object().unwrap().len(), 3, "no paging params");
        }
    }

    #[test]
    fn request_body_keeps_quotes_and_unicode() {
        let query = "\"quoted\" \\ \u{e9}t\u{e9} \u{65e5}\u{672c}";
        let body = build_request_body(query, SearchKind::Album);
        assert_eq!(body["search_text"].as_str(), Some(query));
    }

    // ----- search results -----

    #[test]
    fn albums_fixture_maps_fields() {
        let items = parse_results(RESULTS_ALBUMS, SearchKind::Album);
        assert_eq!(items.len(), 4);
        let first = &items[0];
        assert_eq!(first.item_type, ItemType::Album);
        assert_eq!(first.name, "DAFT PUNK REMIXES (2012)");
        assert_eq!(first.band_name, "Daft Punk");
        assert_eq!(first.item_url_path, "https://devon-wells.bandcamp.com/album/daft-punk-remixes-2012");
        assert_eq!(first.img, "https://f4.bcbits.com/img/a1857672052_3.jpg");
        assert_eq!(first.platform, Platform::Bandcamp);
        assert_eq!(first.year, None);
        assert_eq!(first.duration, None);
        assert_eq!(first.view_count, None);
        assert_eq!(first.subscriber_count, None);
        assert_eq!(first.album_name, None);
        // "Daft Punk, Ghoulie" band name is kept as-is.
        assert_eq!(items[2].band_name, "Daft Punk, Ghoulie");
    }

    #[test]
    fn tracks_fixture_maps_fields() {
        let items = parse_results(RESULTS_TRACKS, SearchKind::Track);
        assert_eq!(items.len(), 4);
        let first = &items[0];
        assert_eq!(first.item_type, ItemType::Track);
        assert_eq!(first.name, "Daft Punk (Freejak's Tribute Mix)");
        assert_eq!(first.band_name, "Daft Punk");
        assert_eq!(first.item_url_path, "https://freejak.bandcamp.com/track/daft-punk-freejaks-tribute-mix");
        assert_eq!(first.img, "https://f4.bcbits.com/img/a1024491655_3.jpg");
        assert!(items.iter().all(|i| i.item_type == ItemType::Track));
    }

    #[test]
    fn artists_fixture_uses_root_as_url_and_keeps_image() {
        let items = parse_results(RESULTS_ARTISTS, SearchKind::Artist);
        assert_eq!(items.len(), 3);
        let first = &items[0];
        assert_eq!(first.item_type, ItemType::Artist);
        assert_eq!(first.name, "Shitty Daft Punk");
        assert_eq!(first.band_name, "Shitty Daft Punk", "artists have no band_name: falls back to name");
        assert_eq!(first.item_url_path, "https://shittydaftpunk.bandcamp.com");
        assert_eq!(first.img, "https://f4.bcbits.com/img/0015565665_23.jpg");
    }

    #[test]
    fn kind_mismatch_is_filtered_out() {
        assert!(parse_results(RESULTS_ALBUMS, SearchKind::Track).is_empty());
        assert!(parse_results(RESULTS_ALBUMS, SearchKind::Artist).is_empty());
        assert!(parse_results(RESULTS_TRACKS, SearchKind::Album).is_empty());
        assert!(parse_results(RESULTS_ARTISTS, SearchKind::Track).is_empty());
    }

    #[test]
    fn unexpected_param_error_body_is_empty() {
        for kind in ALL_KINDS {
            assert!(parse_results(UNEXPECTED_PARAM, kind).is_empty());
        }
    }

    #[test]
    fn garbage_bodies_are_empty() {
        for body in [
            "",
            "   ",
            "not json",
            "<html>challenge</html>",
            "[]",
            "{}",
            "null",
            r#"{"auto":{}}"#,
            r#"{"auto":{"results":5}}"#,
            r#"{"auto":{"results":["x", 3, null]}}"#,
        ] {
            for kind in ALL_KINDS {
                assert!(parse_results(body, kind).is_empty(), "body {body:?}");
            }
        }
    }

    #[test]
    fn rows_missing_fields_are_skipped() {
        let body = r#"{"auto":{"results":[
            {"type":"a","name":"No url","img":"x"},
            {"type":"a","item_url_path":"https://x.bandcamp.com/album/nameless"},
            {"type":"a","name":"   ","item_url_path":"https://x.bandcamp.com/album/blank"},
            {"type":"a","name":"Ok","item_url_path":"https://ok.bandcamp.com/album/ok"},
            {"type":"a","name":"Not an object"}
        ]}}"#;
        let items = parse_results(body, SearchKind::Album);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Ok");
        assert_eq!(items[0].img, "", "missing image stays empty");
        assert_eq!(items[0].band_name, "Ok", "missing band falls back to name");
    }

    #[test]
    fn artist_without_root_is_skipped() {
        let body = r#"{"auto":{"results":[
            {"type":"b","name":"Has path only","item_url_path":"https://x.bandcamp.com"}
        ]}}"#;
        assert!(parse_results(body, SearchKind::Artist).is_empty());
    }

    #[test]
    fn track_album_name_unicode_and_dedup() {
        let body = r#"{"auto":{"results":[
            {"type":"t","name":"Été 日本","band_name":"Ärger","album_name":"Älbum 日","item_url_path":"https://a.bandcamp.com/track/t1","img":"https://f4.bcbits.com/img/1_3.jpg"},
            {"type":"t","name":"Duplicate","band_name":"x","album_name":null,"item_url_path":"https://a.bandcamp.com/track/t1","img":""},
            {"type":"t","name":"Other","band_name":null,"album_name":null,"item_url_path":"https://a.bandcamp.com/track/t2","img":"https://f4.bcbits.com/img/2_3.jpg"}
        ]}}"#;
        let items = parse_results(body, SearchKind::Track);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "\u{c9}t\u{e9} \u{65e5}\u{672c}");
        assert_eq!(items[0].band_name, "\u{c4}rger");
        assert_eq!(items[0].album_name.as_deref(), Some("\u{c4}lbum \u{65e5}"));
        assert_eq!(items[0].img, "https://f4.bcbits.com/img/a1_3.jpg");
        assert_eq!(items[1].album_name, None);
        assert_eq!(items[1].band_name, "Other");
    }

    // ----- url helpers -----

    #[test]
    fn site_root_keeps_scheme_and_host_only() {
        assert_eq!(
            site_root("https://shittydaftpunk.bandcamp.com/music").as_deref(),
            Some("https://shittydaftpunk.bandcamp.com")
        );
        assert_eq!(
            site_root("https://x.bandcamp.com/album/y?z=1#q").as_deref(),
            Some("https://x.bandcamp.com")
        );
        assert_eq!(
            site_root("  https://BANDCAMP.com/ ").as_deref(),
            Some("https://bandcamp.com")
        );
        assert_eq!(
            site_root("https://a.b.bandcamp.com/music").as_deref(),
            Some("https://a.b.bandcamp.com")
        );
    }

    #[test]
    fn site_root_rejects_other_input() {
        for url in ["", "http://x.bandcamp.com", "ftp://x", "javascript:alert(1)", "not a url", "https://"] {
            assert_eq!(site_root(url), None, "url {url:?}");
        }
    }

    #[test]
    fn site_root_rejects_non_bandcamp_hosts() {
        // Custom domains are refused on purpose (the url comes from the frontend).
        for url in [
            "https://custom-domain.example/",
            "https://merch.example.com/music",
            "https://bandcamp.com.evil.com/music",
            "https://evilbandcamp.com/",
            "https://xbandcamp.com/",
            "https://evil.com/x.bandcamp.com",
            "https://bandcamp.com@evil.com/",
            "https://127.0.0.1/",
            "https://localhost/",
        ] {
            assert_eq!(site_root(url), None, "url {url:?}");
        }
    }

    #[test]
    fn is_bandcamp_host_matches_only_bandcamp_domains() {
        assert!(is_bandcamp_host("bandcamp.com"));
        assert!(is_bandcamp_host("x.bandcamp.com"));
        assert!(!is_bandcamp_host(""));
        assert!(!is_bandcamp_host("notbandcamp.com"));
        assert!(!is_bandcamp_host("bandcamp.com.evil.com"));
        assert!(!is_bandcamp_host("x.bandcamp.com.evil.com"));
    }

    #[test]
    fn artist_profile_rejects_bad_url_without_network() {
        assert!(block_on(artist_profile("ftp://x.bandcamp.com")).is_err());
        assert!(block_on(artist_profile("")).is_err());
    }

    // ----- artist page -----

    #[test]
    fn artist_page_fixture_name_location_avatar_and_albums() {
        let profile = parse_artist_page(ARTIST_PAGE, "https://shittydaftpunk.bandcamp.com");
        assert_eq!(profile.name, "Shitty Daft Punk");
        assert_eq!(profile.location.as_deref(), Some("Minneapolis, Minnesota"));
        assert_eq!(
            profile.avatar.as_deref(),
            Some("https://f4.bcbits.com/img/0015565665_23.jpg")
        );
        assert_eq!(profile.platform, Some(Platform::Bandcamp));
        assert_eq!(profile.url, "https://shittydaftpunk.bandcamp.com");
        assert_eq!(profile.subscribers, None);
        assert_eq!(profile.monthly_audience, None);
        assert_eq!(profile.banner, None);
        assert!(profile.top_songs.is_empty());
        assert!(profile.singles.is_empty());
        assert_eq!(profile.description, None, "fixture bio has no paragraph");

        assert_eq!(profile.albums.len(), 2);
        let first = &profile.albums[0];
        assert_eq!(first.item_type, ItemType::Album);
        assert_eq!(first.name, "A Live Show 2 (performed at Yard Sard #2)");
        assert_eq!(first.band_name, "Shitty Daft Punk");
        assert_eq!(
            first.item_url_path,
            "https://shittydaftpunk.bandcamp.com/album/a-live-show-2-performed-at-yard-sard-2"
        );
        assert_eq!(first.img, "https://f4.bcbits.com/img/a2837406409_16.jpg");
        assert_eq!(first.platform, Platform::Bandcamp);
        assert_eq!(profile.albums[1].name, "music is my shit EP");
    }

    #[test]
    fn artist_page_first_tracks_become_top_songs() {
        let mut html = String::from(r#"<p id="band-name-location"><span class="title">Band</span></p><div id="music-grid">"#);
        for i in 1..=7 {
            html.push_str(&format!(
                r#"<li data-item-id="track-{i}" class="x"><a href="/track/t{i}"><div class="art"><img src="https://f4.bcbits.com/img/a{i}.jpg" /></div><p class="title">Song {i}</p></a></li>"#
            ));
        }
        html.push_str(r#"<li data-item-id="album-9"><a href="/album/a"><img src="https://f4.bcbits.com/img/a9.jpg"><p class="title">Alb</p></a></li></div>"#);
        let profile = parse_artist_page(&html, "https://band.bandcamp.com");
        assert_eq!(profile.top_songs.len(), 5);
        assert_eq!(profile.top_songs[0].name, "Song 1");
        assert_eq!(profile.top_songs[0].item_type, ItemType::Track);
        assert_eq!(profile.singles.len(), 2);
        assert_eq!(profile.singles[0].name, "Song 6");
        assert_eq!(profile.albums.len(), 1);
    }

    #[test]
    fn artist_page_entities_are_decoded() {
        let html = r#"<p id="band-name-location"><span class="title">Mix &amp; &#39;Match&#39;</span><span class="location secondaryText">Berlin &amp; Co</span></p>"#;
        let profile = parse_artist_page(html, "https://mix.bandcamp.com");
        assert_eq!(profile.name, "Mix & 'Match'");
        assert_eq!(profile.location.as_deref(), Some("Berlin & Co"));
    }

    #[test]
    fn artist_page_name_falls_back_to_og_title() {
        let html = r#"<meta property="og:title" content="Fallback &amp; Co"><meta property="og:image" content="https://f4.bcbits.com/img/9_23.jpg">"#;
        let profile = parse_artist_page(html, "https://fallback.bandcamp.com");
        assert_eq!(profile.name, "Fallback & Co");
        assert_eq!(profile.avatar.as_deref(), Some("https://f4.bcbits.com/img/9_23.jpg"));
    }

    #[test]
    fn artist_page_cross_domain_href_stays_absolute() {
        let html = r#"<p id="band-name-location"><span class="title">A</span></p>
        <ol id="music-grid">
          <li data-item-id="album-1" class="music-grid-item">
            <a href="https://other.bandcamp.com/album/x"><div class="art"><img src="https://f4.bcbits.com/img/a1_16.jpg" alt="" /></div><p class="title">X</p></a>
          </li>
        </ol>"#;
        let profile = parse_artist_page(html, "https://a.bandcamp.com");
        assert_eq!(profile.albums.len(), 1);
        assert_eq!(profile.albums[0].item_url_path, "https://other.bandcamp.com/album/x");
        assert_eq!(profile.albums[0].img, "https://f4.bcbits.com/img/a1_16.jpg");
    }

    #[test]
    fn artist_page_tracks_go_to_top_songs_and_titles_unescape() {
        let html = r#"<p id="band-name-location"><span class="title">A</span></p>
        <ol id="music-grid">
          <li data-item-id="track-99" class="music-grid-item">
            <a href="/track/t-and-u"><div class="art"><img src="https://f4.bcbits.com/img/t_16.jpg" alt="" /></div>
            <p class="title">
              T &amp; U
            </p></a>
          </li>
          <li data-item-id="merch-1"><a href="/merch/x"><p class="title">Shirt</p></a></li>
          <li data-item-id="album-2"><a href="/album/no-title"><img src="x.jpg"/></a></li>
        </ol>"#;
        let profile = parse_artist_page(html, "https://a.bandcamp.com");
        assert!(profile.albums.is_empty());
        assert_eq!(profile.top_songs.len(), 1);
        let single = &profile.top_songs[0];
        assert_eq!(single.item_type, ItemType::Track);
        assert_eq!(single.name, "T & U");
        assert_eq!(single.item_url_path, "https://a.bandcamp.com/track/t-and-u");
    }

    #[test]
    fn artist_page_bio_paragraphs_are_joined() {
        let html = r#"<div id="bio-container"><h3 class="title">about</h3><p>Line one.</p><p>Line &amp; <b>two</b>.</p></div><ol id="music-grid"></ol>"#;
        let profile = parse_artist_page(html, "https://a.bandcamp.com");
        assert_eq!(profile.description.as_deref(), Some("Line one.\n\nLine & two ."));
    }

    #[test]
    fn artist_page_garbage_gives_empty_profile() {
        for html in ["", "<html></html>", "<li data-item-id=\"album-1\"", "\u{0}\u{1}\u{2}"] {
            let profile = parse_artist_page(html, "https://a.bandcamp.com");
            assert_eq!(profile.name, "");
            assert_eq!(profile.location, None);
            assert_eq!(profile.avatar, None);
            assert!(profile.albums.is_empty());
            assert!(profile.singles.is_empty());
        }
    }

    #[test]
    fn artist_page_bad_base_url_keeps_absolute_hrefs_only() {
        let html = r#"<p id="band-name-location"><span class="title">A</span></p>
        <li data-item-id="album-1"><a href="/album/rel"><p class="title">Rel</p></a></li>
        <li data-item-id="album-2"><a href="https://abs.example/album/abs"><p class="title">Abs</p></a></li>"#;
        let profile = parse_artist_page(html, "not a base");
        assert_eq!(profile.albums.len(), 1);
        assert_eq!(profile.albums[0].name, "Abs");
    }

    #[test]
    fn decode_and_clean_helpers() {
        assert_eq!(decode("  a &amp; b  "), "a & b");
        assert_eq!(clean_text("  <b>Bold</b>\n &amp;   text  "), "Bold & text");
        assert_eq!(clean_text(""), "");
    }
}
