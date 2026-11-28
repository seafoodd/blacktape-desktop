use super::collect_candidate_results;
use crate::{search::ItemType, search::SearchSuggestion, types::Platform};
use tokio::task::JoinSet;

fn suggestion(name: &str) -> SearchSuggestion {
    SearchSuggestion {
        item_type: ItemType::Track,
        name: name.into(),
        band_name: "Artist".into(),
        album_name: None,
        item_url_path: format!("https://example.test/{name}"),
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

#[tokio::test]
async fn all_failed_candidate_sources_return_an_error() {
    let mut jobs = JoinSet::new();
    jobs.spawn(async { Err::<Vec<SearchSuggestion>, _>("YouTube failed".to_string()) });
    jobs.spawn(async { Err::<Vec<SearchSuggestion>, _>("SoundCloud failed".to_string()) });

    let error = collect_candidate_results(jobs).await.unwrap_err();
    assert!(error.contains("YouTube failed"), "unexpected error: {error}");
    assert!(error.contains("SoundCloud failed"), "unexpected error: {error}");
}

#[tokio::test]
async fn successful_empty_candidate_sources_return_an_empty_result() {
    let mut jobs = JoinSet::new();
    jobs.spawn(async { Ok::<Vec<SearchSuggestion>, String>(Vec::new()) });
    jobs.spawn(async { Ok::<Vec<SearchSuggestion>, String>(Vec::new()) });

    assert!(collect_candidate_results(jobs).await.unwrap().is_empty());
}

#[tokio::test]
async fn successful_candidates_are_kept_when_another_source_fails() {
    let mut jobs = JoinSet::new();
    jobs.spawn(async { Err::<Vec<SearchSuggestion>, _>("SoundCloud failed".to_string()) });
    jobs.spawn(async { Ok::<Vec<SearchSuggestion>, String>(vec![suggestion("Found track")]) });

    let results = collect_candidate_results(jobs).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Found track");
}
