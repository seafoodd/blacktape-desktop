use super::{collect_suggestions, ItemType, SearchSuggestion, SearchResult};
use crate::types::Platform;
use std::time::Duration;
use tokio::{sync::{mpsc, oneshot}, task::JoinSet, time::timeout};

fn suggestion(name: &str, platform: Platform) -> SearchSuggestion {
    SearchSuggestion {
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
        platform,
    }
}

fn names(items: &[SearchSuggestion]) -> Vec<String> {
    let mut names: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn emits_partial_after_first_fast_source_while_another_fast_source_is_gated() {
    let mut fast: JoinSet<SearchResult> = JoinSet::new();
    fast.spawn(async { Ok(vec![suggestion("fast first", Platform::Youtube)]) });
    let (fast_gate_tx, fast_gate_rx) = oneshot::channel::<()>();
    fast.spawn(async move {
        let _ = fast_gate_rx.await;
        Ok(vec![suggestion("fast second", Platform::Bandcamp)])
    });

    let mut slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
    let (slow_gate_tx, slow_gate_rx) = oneshot::channel::<()>();
    slow.spawn(async move {
        let _ = slow_gate_rx.await;
        vec![suggestion("slow", Platform::Soundcloud)]
    });

    let (partial_tx, mut partial_rx) = mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        collect_suggestions(fast, slow, |items| {
            let _ = partial_tx.send(names(items));
        })
        .await
    });

    let partial = timeout(Duration::from_secs(1), partial_rx.recv())
        .await
        .expect("first fast source should produce partial results")
        .expect("partial callback should send a result");
    assert_eq!(partial, vec!["fast first"]);
    let _ = slow_gate_tx.send(());
    let _ = fast_gate_tx.send(());
    let final_items = collector.await.expect("collector task should finish");
    assert_eq!(
        names(&final_items),
        vec!["fast first", "fast second", "slow"]
    );
}

#[tokio::test]
async fn emits_musicbrainz_partial_before_a_gated_fast_source_finishes() {
    let mut fast: JoinSet<SearchResult> = JoinSet::new();
    let (fast_gate_tx, fast_gate_rx) = oneshot::channel::<()>();
    fast.spawn(async move {
        let _ = fast_gate_rx.await;
        Ok(vec![suggestion("fast", Platform::Youtube)])
    });

    let mut slow: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
    slow.spawn(async { vec![suggestion("musicbrainz", Platform::Local)] });

    let (partial_tx, mut partial_rx) = mpsc::unbounded_channel();
    let collector = tokio::spawn(async move {
        collect_suggestions(fast, slow, |items| {
            let _ = partial_tx.send(names(items));
        })
        .await
    });

    let partial = timeout(Duration::from_secs(1), partial_rx.recv())
        .await
        .expect("MusicBrainz result should produce partial results")
        .expect("partial callback should send a result");
    assert_eq!(partial, vec!["musicbrainz"]);
    let _ = fast_gate_tx.send(());
    let final_items = collector.await.expect("collector task should finish");
    assert_eq!(names(&final_items), vec!["fast", "musicbrainz"]);
}
