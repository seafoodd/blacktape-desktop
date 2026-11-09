//! Full search page backend (search v2): types, orchestration and the logic behind the
//! `search_full` and `get_artist_profile` Tauri commands (the thin `#[command]` wrappers live in `lib.rs`).
//!
//! OWNER: part B (backend-full). Contract = C:\Dev\blacktape-search-work\v2\DECISIONS.md (D1-D20).
//! The serialized shape of every type here is MIRRORED in `src/shared/lib/searchFull.ts`; it is FROZEN:
//! do not rename/remove fields (additive optional fields only if the architect agrees).
//!
//! Model: the page asks for ONE item kind at a time (the frontend fires Track, Album and Artist requests
//! in parallel on first load, then "Show more" per kind with the cursor returned by the previous page).
//!
//! * First page (`cursor == None`): YouTube Music is fetched by following continuation tokens
//!   sequentially until at least `first_page_target(kind)` raw items were collected or the chain ends
//!   (tracks 50 -> 3 requests of 20, albums 50 -> 3, artists 30 -> 2). Bandcamp is fetched with ONE
//!   filtered request (50 albums / 50 tracks / 10 artists max, no paging exists, see D9). Both run
//!   concurrently. Nothing is truncated afterwards (a truncated YT item would be lost forever because the
//!   cursor points past it); the result is therefore typically ~60 + ~50 raw items per kind.
//! * "Show more" (`cursor == Some(c)`): exactly ONE YouTube continuation request using `c.youtube`
//!   (20 items). Bandcamp is NOT queried again. If `c.youtube` is `None` the page is empty.
//! * Items of the page are ranked with `ranking::rank_flat` (cross-platform dedup + score sort) and returned.
//!   Later pages are ranked among themselves and APPENDED by the frontend (it dedups by `item_url_path`).
//! * Failure policy: a platform that fails contributes a `warnings` entry and no items. `Err(String)` is
//!   returned only when every requested platform failed (or a request was invalid).

#![allow(dead_code)]

use crate::search::{bandcamp_full, ranking, soundcloud, youtube_full, SearchSuggestion};
use crate::types::Platform;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Duration;
use tokio::time::error::Elapsed;

/// Which kind of item a `search_full` request is for. Serialized as "Track" | "Album" | "Artist"
/// (same strings as `bandcamp::ItemType`, so the frontend can reuse its `ItemType` enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchKind {
    Track,
    Album,
    Artist,
}

/// Opaque paging state, round-tripped by the frontend: take `FullSearchPage::next_cursor` of the previous
/// page and send it back unchanged. Bandcamp has no paging so it has no field here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCursor {
    /// YouTube Music continuation token (the raw string from `nextContinuationData.continuation`,
    /// contains `%3D` escapes: use it verbatim in the URL, do NOT percent-encode it again).
    #[serde(default)]
    pub youtube: Option<String>,
    /// SoundCloud `offset` of the next page.
    #[serde(default)]
    pub soundcloud: Option<u32>,
}

/// A non-fatal platform problem, shown by the page as a small notice ("YouTube is unavailable").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformWarning {
    pub platform: Platform,
    pub message: String,
}

/// One page of results for one kind.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FullSearchPage {
    /// Ranked, deduplicated items, all of the requested kind (`SearchSuggestion` shape unchanged, D8).
    pub items: Vec<SearchSuggestion>,
    /// Cursor for the next page, `None` when there is nothing more. Invariant: `has_more == next_cursor.is_some()`.
    pub next_cursor: Option<SearchCursor>,
    pub has_more: bool,
    /// Platforms that failed while the others succeeded.
    pub warnings: Vec<PlatformWarning>,
}

/// Profile card data for an artist (best matching artist of the query). Fields a platform cannot provide
/// are `None` / empty. Serialized with snake_case field names.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ArtistProfile {
    pub platform: Option<Platform>,
    pub name: String,
    /// Canonical artist url: YouTube `https://youtube.com/browse/{UC..}`, Bandcamp `https://x.bandcamp.com`.
    pub url: String,
    /// Square avatar when the platform gives one. YouTube Music: `None` (the header image is a wide banner;
    /// the frontend falls back to the `img` of the search item). Bandcamp: `og:image`.
    pub avatar: Option<String>,
    /// Wide header image (YouTube Music immersive header). Optional decoration.
    pub banner: Option<String>,
    /// YouTube: header "7.21M" subscribers parsed to a number; Bandcamp/SoundCloud: `None`.
    pub subscribers: Option<u64>,
    /// SoundCloud follower count; other platforms `None`.
    pub followers: Option<u64>,
    /// YouTube: "67.4M monthly audience" parsed; Bandcamp: `None`.
    pub monthly_audience: Option<u64>,
    /// Bio text (YouTube description / Bandcamp bio when present), trimmed, may be long.
    pub description: Option<String>,
    /// Bandcamp "Minneapolis, Minnesota"; YouTube `None`.
    pub location: Option<String>,
    /// YouTube "Top songs" shelf (<= 5), `item_type = Track`, `band_name` = artist names, `album_name` set,
    /// `view_count` from "1.2B plays". No duration. Bandcamp: empty.
    pub top_songs: Vec<SearchSuggestion>,
    /// YouTube "Albums" carousel (<= 10) / Bandcamp grid entries `album-*`; `item_type = Album`, year if known.
    pub albums: Vec<SearchSuggestion>,
    /// YouTube "Singles & EPs" carousel (<= 10), `item_type = Album`. Bandcamp grid entries `track-*` as
    /// `item_type = Track`. Page renders them as a second row.
    pub singles: Vec<SearchSuggestion>,
}

/// Number of raw YouTube items the first page tries to collect per kind (D5).
pub fn first_page_target(kind: SearchKind) -> usize {
    match kind {
        SearchKind::Track | SearchKind::Album => 50,
        SearchKind::Artist => 30,
    }
}

/// Timeout of the Bandcamp search request (D18).
const BANDCAMP_TIMEOUT: Duration = Duration::from_secs(4);
/// Timeout of the artist profile request (D18).
const PROFILE_TIMEOUT: Duration = Duration::from_secs(6);
/// Timeout of one SoundCloud search request.
const SOUNDCLOUD_TIMEOUT: Duration = Duration::from_secs(5);

/// What one platform contributed to a page. Built from a successful platform call.
#[derive(Debug, Default, Clone)]
pub(crate) struct Fetched {
    pub items: Vec<SearchSuggestion>,
    /// YouTube only: the token of the next page. Bandcamp always leaves it `None`.
    pub next_token: Option<String>,
    /// SoundCloud only: the offset of the next page.
    pub next_offset: Option<u32>,
    /// Partial failure (YouTube first-page chain): items kept, `next_token` is the token that failed.
    pub warning: Option<String>,
}

/// Orchestrates one `search_full` request (see module docs).
///
/// Contract:
/// * `platforms` may contain Youtube and/or Bandcamp (Local and duplicates ignored). Empty query
///   (after trim) or no usable platform -> `Ok(FullSearchPage::default())`.
/// * Without a cursor (first page): YouTube (chain, bounded by its own 8 s budget) and Bandcamp (one request,
///   4 s) run concurrently. With a cursor ("Show more"): exactly one YouTube continuation request (4 s), no Bandcamp.
///   A cursor without a YouTube token, or a request without YouTube, gives an empty page.
/// * Result = `ranking::rank_flat(all_items, query)`; `next_cursor` is `Some` iff YouTube returned a continuation
///   token (D6); `has_more` mirrors it.
/// * One platform failing -> warning, other platform's items still returned. All requested platforms
///   failing -> `Err(message)`.
pub async fn search_full(
    query: &str,
    kind: SearchKind,
    platforms: &[Platform],
    cursor: Option<SearchCursor>,
) -> Result<FullSearchPage, String> {
    let query = query.trim();
    let want_youtube = platforms.contains(&Platform::Youtube);
    let want_bandcamp = platforms.contains(&Platform::Bandcamp);
    let want_soundcloud = platforms.contains(&Platform::Soundcloud);
    if query.is_empty() || !(want_youtube || want_bandcamp || want_soundcloud) {
        return Ok(FullSearchPage::default());
    }

    if let Some(cursor) = cursor {
        // "Show more": one continuation request per platform that has a cursor. Bandcamp has no paging (D9).
        let token = cursor
            .youtube
            .filter(|token| want_youtube && !token.is_empty());
        let offset = cursor.soundcloud.filter(|_| want_soundcloud);
        if token.is_none() && offset.is_none() {
            return Ok(FullSearchPage::default());
        }
        let youtube = async {
            match &token {
                Some(token) => Some(youtube_more(query, kind, token).await),
                None => None,
            }
        };
        let soundcloud = async {
            match offset {
                Some(offset) => Some(soundcloud_page(query, kind, offset).await),
                None => None,
            }
        };
        let (youtube, soundcloud) = tokio::join!(youtube, soundcloud);
        return assemble_platforms(query, youtube, None, soundcloud);
    }

    let youtube = async {
        if want_youtube {
            Some(youtube_first_page(query, kind).await)
        } else {
            None
        }
    };
    let bandcamp = async {
        if want_bandcamp {
            Some(bandcamp_page(query, kind).await)
        } else {
            None
        }
    };
    let soundcloud = async {
        if want_soundcloud {
            Some(soundcloud_page(query, kind, 0).await)
        } else {
            None
        }
    };
    let (youtube, bandcamp, soundcloud) = tokio::join!(youtube, bandcamp, soundcloud);
    assemble_platforms(query, youtube, bandcamp, soundcloud)
}

/// First YouTube page of `kind` (D5 chain). Err only when its very first request failed.
async fn youtube_first_page(query: &str, kind: SearchKind) -> Result<Fetched, String> {
    youtube_full::fetch_first_page(query, kind, first_page_target(kind))
        .await
        .map(|page| Fetched {
            items: page.items,
            next_token: page.next_token,
            warning: page.warning,
            next_offset: None,
        })
        .map_err(|e| e.to_string())
}

/// One YouTube continuation page ("Show more"), 20 raw items.
async fn youtube_more(query: &str, kind: SearchKind, token: &str) -> Result<Fetched, String> {
    youtube_full::fetch_page(query, kind, Some(token))
        .await
        .map(|page| Fetched {
            items: page.items,
            next_token: page.next_token,
            warning: None,
            next_offset: None,
        })
        .map_err(|e| e.to_string())
}

/// Bandcamp filtered search for `kind` (first page only, D9), bounded by `BANDCAMP_TIMEOUT`.
async fn bandcamp_page(query: &str, kind: SearchKind) -> Result<Fetched, String> {
    match tokio::time::timeout(BANDCAMP_TIMEOUT, bandcamp_full::search_page(query, kind)).await {
        Ok(Ok(items)) => Ok(Fetched {
            items,
            next_token: None,
            warning: None,
            next_offset: None,
        }),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("request timed out".to_string()),
    }
}

/// One SoundCloud page of `kind` starting at `offset`, bounded by `SOUNDCLOUD_TIMEOUT`.
async fn soundcloud_page(query: &str, kind: SearchKind, offset: u32) -> Result<Fetched, String> {
    let sc_kind = match kind {
        SearchKind::Track => soundcloud::SearchKind::Tracks,
        SearchKind::Album => soundcloud::SearchKind::Sets,
        SearchKind::Artist => soundcloud::SearchKind::Users,
    };
    let limit = first_page_target(kind) as u32;
    match tokio::time::timeout(
        SOUNDCLOUD_TIMEOUT,
        soundcloud::search_kind(query, sc_kind, limit, offset),
    )
    .await
    {
        Ok(Ok(page)) => Ok(Fetched {
            items: page.items,
            next_offset: page.next_offset,
            ..Fetched::default()
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("request timed out".to_string()),
    }
}

/// Merges the platform outcomes of one request into a page. Pure, so the merge rules can be tested with fake inputs.
///
/// * `None` = the platform was not requested. No requested platform -> empty page.
/// * An `Err` counts as a failed platform (warning). Only when EVERY requested platform failed -> `Err`.
/// * Items of all successful platforms are ranked with `ranking::rank_flat`.
/// * The cursor comes from YouTube only (`next_token`); a YouTube warning (chain failure) is kept as a warning.
pub(crate) fn assemble_page(
    query: &str,
    youtube: Option<Result<Fetched, String>>,
    bandcamp: Option<Result<Fetched, String>>,
) -> Result<FullSearchPage, String> {
    assemble_platforms(query, youtube, bandcamp, None)
}

/// `assemble_page` with SoundCloud. The cursor carries the YouTube token and/or the SoundCloud offset.
pub(crate) fn assemble_platforms(
    query: &str,
    youtube: Option<Result<Fetched, String>>,
    bandcamp: Option<Result<Fetched, String>>,
    soundcloud: Option<Result<Fetched, String>>,
) -> Result<FullSearchPage, String> {
    let requested: Vec<(Platform, Result<Fetched, String>)> = [
        (Platform::Youtube, youtube),
        (Platform::Bandcamp, bandcamp),
        (Platform::Soundcloud, soundcloud),
    ]
    .into_iter()
    .filter_map(|(platform, outcome)| outcome.map(|outcome| (platform, outcome)))
    .collect();
    if requested.is_empty() {
        return Ok(FullSearchPage::default());
    }
    let total = requested.len();

    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    let mut next_token = None;
    let mut next_offset = None;
    for (platform, outcome) in requested {
        match outcome {
            Ok(fetched) => {
                items.extend(fetched.items);
                match platform {
                    Platform::Youtube => next_token = fetched.next_token,
                    Platform::Soundcloud => next_offset = fetched.next_offset,
                    _ => {}
                }
                if let Some(message) = fetched.warning {
                    warnings.push(PlatformWarning { platform, message });
                }
            }
            Err(message) => {
                failures.push(format!("{}: {message}", platform_name(platform)));
                warnings.push(PlatformWarning { platform, message });
            }
        }
    }
    if failures.len() == total {
        return Err(failures.join("; "));
    }

    let next_cursor = (next_token.is_some() || next_offset.is_some()).then(|| SearchCursor {
        youtube: next_token,
        soundcloud: next_offset,
    });
    let has_more = next_cursor.is_some();
    Ok(FullSearchPage {
        items: ranking::rank_flat(items, query),
        next_cursor,
        has_more,
        warnings,
    })
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Youtube => "YouTube",
        Platform::Bandcamp => "Bandcamp",
        Platform::Soundcloud => "SoundCloud",
        Platform::Local => "Local",
    }
}

/// Fetches the artist profile for `url` of `platform` (see `youtube_full::artist_profile` /
/// `bandcamp_full::artist_profile`). Err(message) for unsupported platform, bad url or network/parse failure
/// (the frontend then simply shows no profile card). Bounded by a 6 s timeout.
pub async fn artist_profile(platform: Platform, url: &str) -> Result<ArtistProfile, String> {
    let result = match platform {
        Platform::Youtube => {
            tokio::time::timeout(PROFILE_TIMEOUT, youtube_full::artist_profile(url)).await
        }
        Platform::Bandcamp => {
            tokio::time::timeout(PROFILE_TIMEOUT, bandcamp_full::artist_profile(url)).await
        }
        Platform::Soundcloud => {
            tokio::time::timeout(PROFILE_TIMEOUT, soundcloud::artist_profile(url))
                .await
                .map(|r| r.map_err(Into::into))
        }
        Platform::Local => return Err("artist profiles are not available for local files".to_string()),
    };
    flatten_profile(result)
}

/// `timeout(result)` -> `Result<profile, message>`. Pure.
fn flatten_profile(
    result: Result<Result<ArtistProfile, Box<dyn Error + Send + Sync>>, Elapsed>,
) -> Result<ArtistProfile, String> {
    match result {
        Ok(Ok(profile)) => Ok(profile),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("artist profile request timed out".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::bandcamp::ItemType;

    #[test]
    fn first_page_targets() {
        assert_eq!(first_page_target(SearchKind::Track), 50);
        assert_eq!(first_page_target(SearchKind::Album), 50);
        assert_eq!(first_page_target(SearchKind::Artist), 30);
    }

    #[test]
    fn kind_serializes_like_item_type() {
        assert_eq!(serde_json::to_string(&SearchKind::Track).unwrap(), "\"Track\"");
        let k: SearchKind = serde_json::from_str("\"Artist\"").unwrap();
        assert_eq!(k, SearchKind::Artist);
    }

    #[test]
    fn cursor_deserializes_with_missing_field() {
        let c: SearchCursor = serde_json::from_str("{}").unwrap();
        assert_eq!(c.youtube, None);
        let c: SearchCursor = serde_json::from_str("{\"youtube\":\"abc%3D\"}").unwrap();
        assert_eq!(c.youtube.as_deref(), Some("abc%3D"));
    }

    #[test]
    fn page_serializes_snake_case() {
        let v = serde_json::to_value(FullSearchPage::default()).unwrap();
        assert!(v.get("next_cursor").is_some());
        assert_eq!(v["has_more"], false);
        assert!(v["items"].as_array().unwrap().is_empty());
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(future)
    }

    fn sug(platform: Platform, item_type: ItemType, name: &str, url: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type,
            name: name.to_string(),
            band_name: "Band".to_string(),
            album_name: None,
            item_url_path: url.to_string(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform,
        }
    }

    fn fetched(items: Vec<SearchSuggestion>, next: Option<&str>, warning: Option<&str>) -> Fetched {
        Fetched {
            items,
            next_token: next.map(str::to_string),
            warning: warning.map(str::to_string),
            next_offset: None,
        }
    }

    fn names(page: &FullSearchPage) -> Vec<String> {
        page.items.iter().map(|i| i.name.clone()).collect()
    }

    fn yt_track(name: &str) -> SearchSuggestion {
        sug(
            Platform::Youtube,
            ItemType::Track,
            name,
            &format!("https://youtube.com/watch?v={name}"),
        )
    }

    fn bc_track(name: &str) -> SearchSuggestion {
        sug(
            Platform::Bandcamp,
            ItemType::Track,
            name,
            &format!("https://band.bandcamp.com/track/{name}"),
        )
    }

    // ----- assemble_page (merge / warning / error / cursor rules) -----

    #[test]
    fn assemble_nothing_requested_is_empty_page() {
        let page = assemble_page("q", None, None).unwrap();
        assert!(page.items.is_empty());
        assert!(page.warnings.is_empty());
        assert!(page.next_cursor.is_none());
        assert!(!page.has_more);
    }

    #[test]
    fn assemble_merges_and_ranks_both_platforms() {
        let yt = vec![yt_track("alpha"), yt_track("beta")];
        let bc = vec![bc_track("gamma")];
        let expected = ranking::rank_flat(yt.iter().cloned().chain(bc.iter().cloned()).collect(), "q");
        let page = assemble_page(
            "q",
            Some(Ok(fetched(yt, None, None))),
            Some(Ok(fetched(bc, None, None))),
        )
        .unwrap();
        assert_eq!(page.items.len(), 3);
        let expected_names: Vec<String> = expected.iter().map(|i| i.name.clone()).collect();
        assert_eq!(names(&page), expected_names, "page order equals rank_flat order");
        assert!(page.warnings.is_empty());
        assert!(!page.has_more);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn assemble_cursor_comes_from_youtube_only() {
        let page = assemble_page(
            "q",
            Some(Ok(fetched(vec![yt_track("a")], Some("yt-token"), None))),
            Some(Ok(fetched(vec![bc_track("b")], Some("bc-token"), None))),
        )
        .unwrap();
        let cursor = page.next_cursor.expect("youtube has more");
        assert_eq!(cursor.youtube.as_deref(), Some("yt-token"));
        assert!(page.has_more);
    }

    #[test]
    fn assemble_cursor_carries_soundcloud_offset() {
        let sc = Fetched {
            items: vec![yt_track("s")],
            next_offset: Some(50),
            ..Fetched::default()
        };
        let page = assemble_platforms("q", None, None, Some(Ok(sc.clone()))).unwrap();
        let cursor = page.next_cursor.expect("soundcloud has more");
        assert_eq!(cursor.soundcloud, Some(50));
        assert_eq!(cursor.youtube, None);
        assert!(page.has_more);

        let both = assemble_platforms(
            "q",
            Some(Ok(fetched(vec![yt_track("a")], Some("t"), None))),
            None,
            Some(Ok(sc)),
        )
        .unwrap();
        let cursor = both.next_cursor.unwrap();
        assert_eq!((cursor.youtube.as_deref(), cursor.soundcloud), (Some("t"), Some(50)));
    }

    #[test]
    fn assemble_soundcloud_failure_is_a_warning_when_another_platform_works() {
        let page = assemble_platforms(
            "q",
            Some(Ok(fetched(vec![yt_track("a")], None, None))),
            None,
            Some(Err("client id".into())),
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.warnings.len(), 1);
        assert_eq!(page.warnings[0].platform, Platform::Soundcloud);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn assemble_all_failed_including_soundcloud_is_error() {
        let err = assemble_platforms("q", Some(Err("a".into())), None, Some(Err("b".into()))).unwrap_err();
        assert!(err.contains("YouTube") && err.contains("SoundCloud"), "{err}");
    }

    #[test]
    fn cursor_deserializes_soundcloud_offset() {
        let c: SearchCursor = serde_json::from_str("{\"soundcloud\":50}").unwrap();
        assert_eq!(c.soundcloud, Some(50));
        assert_eq!(c.youtube, None);
    }

    #[test]
    fn assemble_has_more_mirrors_cursor() {
        let without = assemble_page("q", Some(Ok(fetched(vec![yt_track("a")], None, None))), None).unwrap();
        assert!(!without.has_more);
        assert!(without.next_cursor.is_none());

        let with = assemble_page("q", Some(Ok(fetched(vec![yt_track("a")], Some("t"), None))), None).unwrap();
        assert!(with.has_more);
        assert_eq!(with.next_cursor.unwrap().youtube.as_deref(), Some("t"));
    }

    #[test]
    fn assemble_youtube_error_keeps_bandcamp_items_with_warning() {
        let page = assemble_page(
            "q",
            Some(Err("timed out".to_string())),
            Some(Ok(fetched(vec![bc_track("only")], None, None))),
        )
        .unwrap();
        assert_eq!(names(&page), vec!["only".to_string()]);
        assert_eq!(page.warnings.len(), 1);
        assert_eq!(page.warnings[0].platform, Platform::Youtube);
        assert_eq!(page.warnings[0].message, "timed out");
        assert!(!page.has_more);
    }

    #[test]
    fn assemble_bandcamp_error_keeps_youtube_items_with_warning() {
        let page = assemble_page(
            "q",
            Some(Ok(fetched(vec![yt_track("yt")], Some("t"), None))),
            Some(Err("boom".to_string())),
        )
        .unwrap();
        assert_eq!(names(&page), vec!["yt".to_string()]);
        assert_eq!(page.warnings.len(), 1);
        assert_eq!(page.warnings[0].platform, Platform::Bandcamp);
        assert!(page.has_more, "youtube cursor survives a bandcamp failure");
    }

    #[test]
    fn assemble_all_requested_failing_is_error() {
        let err = assemble_page("q", Some(Err("a".into())), Some(Err("b".into()))).unwrap_err();
        assert!(err.contains("YouTube: a"), "{err}");
        assert!(err.contains("Bandcamp: b"), "{err}");
    }

    #[test]
    fn assemble_single_requested_failure_is_error() {
        // Show more path: only YouTube is requested, so its failure is the whole request.
        let err = assemble_page("q", Some(Err("network".into())), None).unwrap_err();
        assert_eq!(err, "YouTube: network");
    }

    #[test]
    fn assemble_chain_warning_keeps_failed_token_as_cursor() {
        let page = assemble_page(
            "q",
            Some(Ok(fetched(
                vec![yt_track("a"), yt_track("b")],
                Some("failed-token"),
                Some("request failed: 500"),
            ))),
            None,
        )
        .unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.has_more);
        assert_eq!(page.next_cursor.unwrap().youtube.as_deref(), Some("failed-token"));
        assert_eq!(page.warnings.len(), 1);
        assert_eq!(page.warnings[0].platform, Platform::Youtube);
        assert_eq!(page.warnings[0].message, "request failed: 500");
    }

    #[test]
    fn assemble_identical_items_are_deduplicated() {
        let page = assemble_page(
            "q",
            Some(Ok(fetched(vec![yt_track("same"), yt_track("same")], None, None))),
            None,
        )
        .unwrap();
        assert_eq!(page.items.len(), 1);
    }

    #[test]
    fn assemble_unicode_names_survive() {
        let page = assemble_page(
            "q",
            Some(Ok(fetched(vec![yt_track("\u{65e5}\u{672c}\u{8a9e} \u{266a}")], None, None))),
            None,
        )
        .unwrap();
        assert_eq!(names(&page), vec!["\u{65e5}\u{672c}\u{8a9e} \u{266a}".to_string()]);
    }

    // ----- search_full early returns (no network involved) -----

    #[test]
    fn search_full_empty_query_is_empty_page() {
        let page = block_on(search_full("   ", SearchKind::Track, &[Platform::Youtube], None)).unwrap();
        assert!(page.items.is_empty());
        assert!(!page.has_more);
    }

    #[test]
    fn search_full_without_usable_platform_is_empty_page() {
        let none = block_on(search_full("daft punk", SearchKind::Album, &[], None)).unwrap();
        assert!(none.items.is_empty());
        let local = block_on(search_full("daft punk", SearchKind::Album, &[Platform::Local], None)).unwrap();
        assert!(local.items.is_empty());
        assert!(local.warnings.is_empty());
    }

    #[test]
    fn search_full_cursor_without_youtube_token_is_empty_page() {
        let page = block_on(search_full(
            "daft punk",
            SearchKind::Track,
            &[Platform::Youtube],
            Some(SearchCursor::default()),
        ))
        .unwrap();
        assert!(page.items.is_empty());
        assert!(!page.has_more);

        let empty = block_on(search_full(
            "daft punk",
            SearchKind::Track,
            &[Platform::Youtube],
            Some(SearchCursor {
                youtube: Some(String::new()),
                soundcloud: None,
            }),
        ))
        .unwrap();
        assert!(empty.items.is_empty());
    }

    #[test]
    fn search_full_cursor_never_asks_bandcamp() {
        // Bandcamp only + a cursor: nothing to continue, so an empty page and no request.
        let page = block_on(search_full(
            "daft punk",
            SearchKind::Track,
            &[Platform::Bandcamp],
            Some(SearchCursor {
                youtube: Some("tok".to_string()),
                soundcloud: None,
            }),
        ))
        .unwrap();
        assert!(page.items.is_empty());
        assert!(page.warnings.is_empty());
        assert!(!page.has_more);
    }

    // ----- artist profile dispatch / timeouts -----

    #[test]
    fn flatten_profile_passes_success_and_errors() {
        let ok: Result<Result<ArtistProfile, Box<dyn Error + Send + Sync>>, Elapsed> =
            Ok(Ok(ArtistProfile {
                name: "X".to_string(),
                ..Default::default()
            }));
        assert_eq!(flatten_profile(ok).unwrap().name, "X");

        let failed: Result<Result<ArtistProfile, Box<dyn Error + Send + Sync>>, Elapsed> =
            Ok(Err("bad page".into()));
        assert_eq!(flatten_profile(failed).unwrap_err(), "bad page");
    }

    #[test]
    fn flatten_profile_maps_elapsed_to_timeout_message() {
        // A zero timeout around a future that never completes yields a real `Elapsed` without any network.
        let elapsed = block_on(async {
            tokio::time::timeout(Duration::ZERO, std::future::pending::<()>()).await
        });
        assert!(elapsed.is_err());
        let mapped: Result<Result<ArtistProfile, Box<dyn Error + Send + Sync>>, Elapsed> =
            elapsed.map(|()| Ok(ArtistProfile::default()));
        assert_eq!(flatten_profile(mapped).unwrap_err(), "artist profile request timed out");
    }

    #[test]
    fn artist_profile_local_platform_is_unsupported() {
        let err = block_on(artist_profile(Platform::Local, "https://youtube.com/browse/UCabc")).unwrap_err();
        assert!(err.contains("not available"), "{err}");
    }

    #[test]
    fn artist_profile_bad_urls_fail_before_any_request() {
        assert!(block_on(artist_profile(Platform::Youtube, "https://youtube.com/watch?v=x")).is_err());
        assert!(block_on(artist_profile(Platform::Youtube, "")).is_err());
        assert!(block_on(artist_profile(Platform::Bandcamp, "ftp://x.bandcamp.com")).is_err());
        assert!(block_on(artist_profile(Platform::Bandcamp, "")).is_err());
    }
}
