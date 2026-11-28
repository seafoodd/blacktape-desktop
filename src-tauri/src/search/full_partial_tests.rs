use super::{collect_page_sources, Fetched, FullSearchPage};
use crate::{search::{progress::Source, ItemType, SearchSuggestion}, types::Platform};
use std::time::Duration;
use tokio::{sync::{mpsc, oneshot}, task::JoinSet, time::timeout};

fn suggestion(name: &str, item_type: ItemType, platform: Platform) -> SearchSuggestion {
    SearchSuggestion {
        item_type,
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
        platform,
    }
}

fn fetched(item: SearchSuggestion) -> Fetched {
    Fetched { items: vec![item], ..Default::default() }
}

fn names(page: &FullSearchPage) -> Vec<String> {
    let mut names: Vec<String> = page.items.iter().map(|item| item.name.clone()).collect();
    names.sort();
    names
}

#[tokio::test]
async fn emits_partial_when_one_source_finishes_while_another_is_gated() {
    let mut jobs: JoinSet<(Source, Result<Fetched, String>)> = JoinSet::new();
    jobs.spawn(async {
        (
            Source::Bandcamp,
            Ok(fetched(suggestion("Bandcamp result", ItemType::Track, Platform::Bandcamp))),
        )
    });
    let (gate_tx, gate_rx) = oneshot::channel::<()>();
    jobs.spawn(async move {
        let _ = gate_rx.await;
        (
            Source::Youtube,
            Ok(fetched(suggestion("YouTube result", ItemType::Track, Platform::Youtube))),
        )
    });

    let (partial_tx, mut partial_rx) = mpsc::unbounded_channel();
    let callback = move |page: FullSearchPage| {
        let _ = partial_tx.send(names(&page));
    };
    let collection = tokio::spawn(async move { collect_page_sources("song", jobs, &callback).await });

    let partial = timeout(Duration::from_secs(1), partial_rx.recv())
        .await
        .expect("first source should emit partial page")
        .expect("partial callback should send a page");
    assert_eq!(partial, vec!["Bandcamp result"]);
    let _ = gate_tx.send(());
    let final_page = collection.await.expect("collector should finish").unwrap();
    assert_eq!(names(&final_page), vec!["Bandcamp result", "YouTube result"]);
}

#[tokio::test]
async fn musicbrainz_first_partial_contains_canonical_album_before_youtube_finishes() {
    let mut jobs: JoinSet<(Source, Result<Fetched, String>)> = JoinSet::new();
    jobs.spawn(async {
        (
            Source::Musicbrainz,
            Ok(fetched(suggestion("Canonical album", ItemType::Album, Platform::Local))),
        )
    });
    let (gate_tx, gate_rx) = oneshot::channel::<()>();
    jobs.spawn(async move {
        let _ = gate_rx.await;
        (
            Source::Youtube,
            Ok(fetched(suggestion("YouTube album", ItemType::Album, Platform::Youtube))),
        )
    });

    let (partial_tx, mut partial_rx) = mpsc::unbounded_channel();
    let callback = move |page: FullSearchPage| {
        let _ = partial_tx.send(names(&page));
    };
    let collection = tokio::spawn(async move { collect_page_sources("album", jobs, &callback).await });

    let partial = timeout(Duration::from_secs(1), partial_rx.recv())
        .await
        .expect("MusicBrainz result should emit partial page")
        .expect("partial callback should send a page");
    assert_eq!(partial, vec!["Canonical album"]);
    let _ = gate_tx.send(());
    let final_page = collection.await.expect("collector should finish").unwrap();
    assert_eq!(names(&final_page), vec!["Canonical album", "YouTube album"]);
}
