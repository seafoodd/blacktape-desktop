use super::{cache::cached_source, progress::Source, SearchSuggestion, SourceResults};
use crate::{search::ItemType, types::Platform, library_index::sources::TtlCache};
use std::{sync::Mutex, time::{Duration, Instant}};

fn source_results(name: &str, complete: bool) -> SourceResults {
    SourceResults {
        items: vec![SearchSuggestion {
            item_type: ItemType::Track,
            name: name.into(),
            band_name: "Artist".into(),
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
        }],
        complete,
    }
}

fn cache() -> Mutex<TtlCache<Vec<SearchSuggestion>>> {
    Mutex::new(TtlCache::new(Duration::from_secs(300)))
}

#[tokio::test]
async fn only_complete_successes_are_cached_and_keys_are_source_specific() {
    let cache = cache();
    let now = Instant::now();

    let error = cached_source(&cache, Source::Youtube, " Radiohead ", now, async {
        Err::<SourceResults, _>("offline".to_string())
    })
    .await;
    assert!(error.is_err());

    let incomplete = cached_source(&cache, Source::Youtube, "radiohead", now, async {
        Ok(source_results("partial", false))
    })
    .await
    .unwrap();
    assert!(!incomplete.complete);

    let complete = cached_source(&cache, Source::Youtube, " RADIOHEAD ", now, async {
        Ok(source_results("complete", true))
    })
    .await
    .unwrap();
    assert_eq!(complete.items[0].name, "complete");

    let mut request_polled = false;
    let hit = cached_source(&cache, Source::Youtube, "radiohead", now, async {
        request_polled = true;
        Ok(source_results("should not replace", true))
    })
    .await
    .unwrap();
    assert!(!request_polled, "fresh cached result should skip the request");
    assert_eq!(hit.items[0].name, "complete");

    let other_source = cached_source(&cache, Source::Bandcamp, "radiohead", now, async {
        Ok(source_results("bandcamp", true))
    })
    .await
    .unwrap();
    assert_eq!(other_source.items[0].name, "bandcamp");
}

#[tokio::test]
async fn expired_source_result_is_fetched_again() {
    let cache = cache();
    let stored_at = Instant::now();
    cached_source(&cache, Source::Youtube, "radiohead", stored_at, async {
        Ok(source_results("old", true))
    })
    .await
    .unwrap();

    let fetched = cached_source(
        &cache,
        Source::Youtube,
        "radiohead",
        stored_at + Duration::from_secs(301),
        async { Ok(source_results("fresh", true)) },
    )
    .await
    .unwrap();
    assert_eq!(fetched.items[0].name, "fresh");
}
