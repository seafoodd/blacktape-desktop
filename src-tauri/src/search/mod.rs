use crate::search::bandcamp::ItemType;
use crate::types::Platform;
use serde::Serialize;
use std::collections::HashSet;
use std::error::Error;
use std::future::Future;
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Debug, Serialize, Clone)]
pub struct SearchSuggestion {
    pub item_type: ItemType,
    pub name: String,
    pub band_name: String,
    pub album_name: Option<String>,
    pub item_url_path: String,
    pub img: String,

    pub subscriber_count: Option<u64>,
    /// What `subscriber_count` counts: `"subscribers"` or `"monthly_audience"`; None when unknown/not applicable.
    pub audience_kind: Option<&'static str>,
    /// Page url of the album a TRACK belongs to (YouTube Music), so the UI can open the album from a track.
    pub album_url: Option<String>,
    pub view_count: Option<u64>,
    pub year: Option<i32>,
    pub duration: Option<u32>,
    pub platform: Platform,
}

pub mod bandcamp;
pub mod soundcloud;
pub mod bandcamp_full;
pub mod full;
pub mod progress;
pub mod ranking;
pub mod youtube;
pub mod youtube_fast;
pub mod youtube_full;
#[cfg(test)]
mod simulation_tests;

/// Time budget for the fast InnerTube path before the legacy rustypipe path takes over (D6).
const FAST_PATH_TIMEOUT: Duration = Duration::from_millis(3500);

type SearchResult = Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>>;

/// Shorter queries are not sent to MusicBrainz from the search dropdown: they are mostly half-typed words.
const MUSICBRAINZ_MIN_QUERY_CHARS: usize = 3;

pub fn is_long_enough_for_musicbrainz(query: &str) -> bool {
    query.trim().chars().count() >= MUSICBRAINZ_MIN_QUERY_CHARS
}

/// Gathers the suggestions of all sources. When the slow one (MusicBrainz) is still running after the others
/// have answered, their results are handed to `on_partial` first.
pub async fn collect_suggestions(
    mut fast: JoinSet<SearchResult>,
    mut slow: JoinSet<Vec<SearchSuggestion>>,
    on_partial: impl FnOnce(&[SearchSuggestion]),
) -> Vec<SearchSuggestion> {
    let mut found = Vec::new();
    while let Some(result) = fast.join_next().await {
        match result {
            Ok(Ok(results)) => found.extend(results),
            Ok(Err(e)) => tracing::warn!("Platform search error: {e}"),
            Err(e) => tracing::error!("Task join error in search suggestions: {e}"),
        }
    }
    let mut ready = slow.try_join_next();
    if ready.is_none() && !slow.is_empty() && !found.is_empty() {
        on_partial(&found);
    }
    loop {
        let result = match ready.take() {
            Some(result) => result,
            None => match slow.join_next().await {
                Some(result) => result,
                None => return found,
            },
        };
        match result {
            Ok(results) => found.extend(results),
            Err(e) => tracing::error!("Task join error in search suggestions: {e}"),
        }
    }
}

/// Fetches canonical album releases from MusicBrainz `release-group` API (under a strict timeout),
/// preferring primary studio albums and high-edition canonical entries, deduplicating `(artist, title)`
/// pairs, and converting them into `SearchSuggestion` items prefixed with `canonical:rg:<mbid>`.
pub async fn fetch_canonical_album_suggestions_result(
    client: &crate::metadata::MusicBrainzClient,
    query: &str,
    platform: Platform,
    limit: usize,
    timeout: Duration,
) -> Result<Vec<SearchSuggestion>, String> {
    let answer = tokio::time::timeout(timeout, client.search_release_groups(query))
        .await
        .unwrap_or_else(|_| Err("MusicBrainz request timed out after 6s. Please try again.".to_string()));
    let mut groups = match answer {
        Ok(groups) => groups,
        Err(error) => client.saved_release_groups(query).await.ok_or(error)?,
    };
    groups.retain(|rg| !crate::metadata::musicbrainz::is_bootleg_only_group(&rg.releases));
    // Prefer full studio Albums over EPs over Singles, no secondary types, and high release counts.
    groups.sort_by(|a, b| {
        let primary_rank = |p: Option<&str>| match p {
            Some(s) if s.eq_ignore_ascii_case("Album") => 2u8,
            Some(s) if s.eq_ignore_ascii_case("EP") => 1u8,
            _ => 0u8,
        };
        let a_primary = a.secondary_types.is_empty();
        let b_primary = b.secondary_types.is_empty();
        let a_type = primary_rank(a.primary_type.as_deref());
        let b_type = primary_rank(b.primary_type.as_deref());
        let a_weight = a.score.unwrap_or(0) + (a.count.unwrap_or(0).min(25) * 3);
        let b_weight = b.score.unwrap_or(0) + (b.count.unwrap_or(0).min(25) * 3);
        b_primary
            .cmp(&a_primary)
            .then_with(|| b_type.cmp(&a_type))
            .then_with(|| b_weight.cmp(&a_weight))
            .then_with(|| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)))
    });
    let has_album_or_ep = groups.iter().any(|rg| {
        rg.primary_type
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("Album") || p.eq_ignore_ascii_case("EP"))
    });
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for rg in groups {
        let count = rg.count.unwrap_or(0);
        let is_single = rg
            .primary_type
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("Single"));
        // Singles with only 1 edition are tracks, not multi-source studio albums: skip them when
        // actual Albums/EPs exist so they never hijack top Album slots (e.g. "plastic death").
        if is_single && count < 2 && has_album_or_ep {
            continue;
        }
        let is_primary_studio = rg.secondary_types.is_empty()
            && rg
                .primary_type
                .as_deref()
                .map_or(true, |p| p.eq_ignore_ascii_case("Album") || p.eq_ignore_ascii_case("EP"));
        let summary = rg.into_canonical_summary();
        if summary.title.trim().is_empty() || summary.artist.trim().is_empty() {
            continue;
        }
        let key = (
            ranking::normalize(&summary.artist),
            ranking::normalize(&summary.title),
        );
        if !key.0.is_empty() && !key.1.is_empty() && !seen.insert(key) {
            continue;
        }
        // Encode edition prominence into internal view_count proxy (not displayed in UI) so
        // widely released studio albums (>= 3 editions) defeat 1-edition homonyms and compilations.
        let prominence_proxy = if count >= 3 && is_primary_studio {
            Some((count as u64).saturating_mul(25_000))
        } else if count >= 2 {
            Some((count as u64).saturating_mul(2_000))
        } else {
            None
        };
        deduped.push(SearchSuggestion {
            item_type: ItemType::Album,
            name: summary.title,
            band_name: summary.artist,
            album_name: None,
            item_url_path: format!("canonical:{}", summary.id),
            img: summary.cover_url.unwrap_or_default(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: prominence_proxy,
            year: summary.year.map(|y| y as i32),
            duration: None,
            platform,
        });
        if deduped.len() >= limit {
            break;
        }
    }
    Ok(deduped)
}

pub async fn fetch_canonical_album_suggestions(
    query: &str,
    cache_dir: Option<std::path::PathBuf>,
    platform: Platform,
    limit: usize,
    timeout: Duration,
) -> Vec<SearchSuggestion> {
    let client = crate::metadata::MusicBrainzClient::new(cache_dir);
    fetch_canonical_album_suggestions_result(&client, query, platform, limit, timeout)
        .await
        .unwrap_or_default()
}

/// YouTube search with the fast InnerTube path first and the legacy rustypipe path as fallback (D6).
/// The fast path is given `FAST_PATH_TIMEOUT`; see `with_fallback` for the exact rules.
pub async fn search_youtube_with_fallback(query: &str) -> SearchResult {
    with_fallback(
        youtube_fast::search(query),
        youtube::search(query),
        FAST_PATH_TIMEOUT,
    )
    .await
}

/// Runs `fast` under `timeout`. Its result is returned as-is unless it is an error, a timeout, or
/// contains no Track/Album/Artist item (see `should_fallback`); then the reason is logged and `slow`
/// is awaited and its result returned unchanged. `slow` is not polled when `fast` succeeds.
async fn with_fallback<F, G>(fast: F, slow: G, timeout: Duration) -> SearchResult
where
    F: Future<Output = SearchResult>,
    G: Future<Output = SearchResult>,
{
    let reason = match tokio::time::timeout(timeout, fast).await {
        Ok(result) if !should_fallback(&result) => return result,
        Ok(Ok(_)) => "no track, album or artist results".to_string(),
        Ok(Err(e)) => format!("error: {e}"),
        Err(_) => format!("timeout after {} ms", timeout.as_millis()),
    };
    tracing::warn!("youtube_fast fallback: {reason}");
    slow.await
}

/// True when the fast path result is unusable: an error, or no item that is a Track, Album or Artist
/// (an empty list, or only `Unknown` items). Non-empty partial results are NOT a fallback case.
fn should_fallback(result: &SearchResult) -> bool {
    match result {
        Err(_) => true,
        Ok(items) => !items.iter().any(|item| {
            matches!(
                item.item_type,
                ItemType::Track | ItemType::Album | ItemType::Artist
            )
        }),
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn suggestion(item_type: ItemType, name: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type,
            name: name.to_string(),
            band_name: "Artist".to_string(),
            album_name: None,
            item_url_path: String::new(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    fn ok_items(items: Vec<SearchSuggestion>) -> SearchResult {
        Ok(items)
    }

    fn err(msg: &str) -> SearchResult {
        Err(msg.into())
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime")
    }

    #[test]
    fn should_fallback_on_error() {
        assert!(should_fallback(&err("network down")));
    }

    #[test]
    fn should_fallback_on_empty_list() {
        assert!(should_fallback(&ok_items(vec![])));
    }

    #[test]
    fn should_fallback_when_only_unknown_items() {
        let items = vec![
            suggestion(ItemType::Unknown, "Video"),
            suggestion(ItemType::Unknown, "Playlist"),
        ];
        assert!(should_fallback(&ok_items(items)));
    }

    #[test]
    fn should_not_fallback_on_track_album_or_artist() {
        for t in [ItemType::Track, ItemType::Album, ItemType::Artist] {
            assert!(!should_fallback(&ok_items(vec![suggestion(t, "x")])));
        }
    }

    #[test]
    fn should_not_fallback_when_unknown_mixed_with_track() {
        let items = vec![
            suggestion(ItemType::Unknown, "Video"),
            suggestion(ItemType::Track, "One More Time"),
        ];
        assert!(!should_fallback(&ok_items(items)));
    }

    #[test]
    fn should_fallback_keeps_unicode_names_irrelevant() {
        // Name content must not influence the decision; only the item type does.
        assert!(should_fallback(&ok_items(vec![suggestion(
            ItemType::Unknown,
            "Сигур Рос 日本語 🎵"
        )])));
        assert!(!should_fallback(&ok_items(vec![suggestion(
            ItemType::Track,
            "Сигур Рос 日本語 🎵"
        )])));
    }

    #[test]
    fn fast_success_is_returned_and_fallback_not_polled() {
        let rt = runtime();
        let slow_polled = Arc::new(AtomicBool::new(false));
        let flag = slow_polled.clone();
        let result = rt.block_on(with_fallback(
            async { ok_items(vec![suggestion(ItemType::Track, "Fast")]) },
            async move {
                flag.store(true, Ordering::SeqCst);
                ok_items(vec![suggestion(ItemType::Track, "Slow")])
            },
            Duration::from_secs(1),
        ));
        let items = result.expect("fast result");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Fast");
        assert!(!slow_polled.load(Ordering::SeqCst));
    }

    #[test]
    fn fast_error_falls_back_to_slow() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async { err("fast broke") },
                async { ok_items(vec![suggestion(ItemType::Album, "Legacy")]) },
                Duration::from_secs(1),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
    }

    #[test]
    fn fast_empty_falls_back_to_slow() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async { ok_items(vec![]) },
                async { ok_items(vec![suggestion(ItemType::Artist, "Legacy")]) },
                Duration::from_secs(1),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
    }

    #[test]
    fn fast_timeout_falls_back_and_cancels_fast() {
        let rt = runtime();
        let started = std::time::Instant::now();
        let items = rt
            .block_on(with_fallback(
                async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    ok_items(vec![suggestion(ItemType::Track, "Too late")])
                },
                async { ok_items(vec![suggestion(ItemType::Track, "Legacy")]) },
                Duration::from_millis(20),
            ))
            .expect("fallback result");
        assert_eq!(items[0].name, "Legacy");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn fast_partial_result_within_timeout_is_kept() {
        let rt = runtime();
        let items = rt
            .block_on(with_fallback(
                async {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    ok_items(vec![suggestion(ItemType::Track, "Partial")])
                },
                async { ok_items(vec![suggestion(ItemType::Track, "Legacy")]) },
                Duration::from_secs(2),
            ))
            .expect("fast result");
        assert_eq!(items[0].name, "Partial");
    }

    #[test]
    fn fallback_error_is_returned_unchanged() {
        let rt = runtime();
        let result = rt.block_on(with_fallback(
            async { err("fast broke") },
            async { err("legacy broke") },
            Duration::from_secs(1),
        ));
        let msg = result.err().expect("error").to_string();
        assert_eq!(msg, "legacy broke");
    }

    #[test]
    fn fast_error_with_empty_fallback_returns_empty_ok() {
        let rt = runtime();
        let result = rt.block_on(with_fallback(
            async { err("fast broke") },
            async { ok_items(vec![]) },
            Duration::from_secs(1),
        ));
        assert!(result.expect("ok").is_empty());
    }
}

#[cfg(test)]
mod collect_suggestions_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    fn suggestion(item_type: ItemType, name: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type,
            name: name.to_string(),
            band_name: "Artist".to_string(),
            album_name: None,
            item_url_path: String::new(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    #[test]
    fn musicbrainz_needs_three_characters_after_trim() {
        assert!(!is_long_enough_for_musicbrainz("ab"));
        assert!(!is_long_enough_for_musicbrainz("  ab  "));
        assert!(is_long_enough_for_musicbrainz("abc"));
        assert!(is_long_enough_for_musicbrainz("мир"));
        assert!(!is_long_enough_for_musicbrainz("ок"));
    }

    #[tokio::test]
    async fn partial_callback_runs_while_slow_source_is_pending() {
        let (release_slow, slow_waits) = oneshot::channel::<()>();
        let mut fast: JoinSet<SearchResult> = JoinSet::new();
        fast.spawn(async { Ok(vec![suggestion(ItemType::Track, "Fast")]) });
        let mut slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
        slow.spawn(async move {
            // Only finishes once on_partial has run, so the callback is guaranteed to see it pending.
            slow_waits.await.expect("on_partial must release the slow source");
            vec![suggestion(ItemType::Album, "Slow")]
        });

        let partial_names = Arc::new(Mutex::new(Vec::new()));
        let partial_names_in_callback = partial_names.clone();
        let found = collect_suggestions(fast, slow, move |partial| {
            *partial_names_in_callback.lock().unwrap() =
                partial.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
            let _ = release_slow.send(());
        })
        .await;

        assert_eq!(*partial_names.lock().unwrap(), vec!["Fast".to_string()]);
        let mut names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Fast", "Slow"]);
    }

    #[tokio::test]
    async fn partial_callback_not_called_without_slow_source() {
        let mut fast: JoinSet<SearchResult> = JoinSet::new();
        fast.spawn(async { Ok(vec![suggestion(ItemType::Track, "Fast")]) });
        let slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
        let called = Arc::new(AtomicBool::new(false));
        let called_in_callback = called.clone();

        let found = collect_suggestions(fast, slow, move |_| {
            called_in_callback.store(true, Ordering::SeqCst);
        })
        .await;

        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    async fn partial_callback_not_called_when_fast_sources_found_nothing() {
        let mut fast: JoinSet<SearchResult> = JoinSet::new();
        fast.spawn(async { Ok(vec![]) });
        fast.spawn(async { Err("fast source down".into()) });
        let mut slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
        slow.spawn(async { vec![suggestion(ItemType::Album, "Slow")] });
        let called = Arc::new(AtomicBool::new(false));
        let called_in_callback = called.clone();

        let found = collect_suggestions(fast, slow, move |_| {
            called_in_callback.store(true, Ordering::SeqCst);
        })
        .await;

        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Slow");
    }

    #[tokio::test]
    async fn failing_fast_source_keeps_results_of_other_sources() {
        let mut fast: JoinSet<SearchResult> = JoinSet::new();
        fast.spawn(async { Err("boom".into()) });
        fast.spawn(async { Ok(vec![suggestion(ItemType::Artist, "Survivor")]) });
        let slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();

        let found = collect_suggestions(fast, slow, |_| {}).await;

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Survivor");
    }
}
