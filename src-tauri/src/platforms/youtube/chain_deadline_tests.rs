use super::{first_page_with_deadline, ParsedPage};
use crate::{search::{ItemType, SearchSuggestion}, types::Platform};
use std::time::{Duration, Instant};
use tokio::time::sleep;

fn item(name: &str) -> SearchSuggestion {
    SearchSuggestion {
        item_type: ItemType::Track,
        name: name.into(),
        band_name: "Artist".into(),
        album_name: None,
        item_url_path: format!("https://youtube.com/watch?v={name}"),
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

fn page(name: &str, next_token: Option<&str>) -> ParsedPage {
    ParsedPage {
        items: vec![item(name)],
        next_token: next_token.map(str::to_string),
        warning: None,
    }
}

#[tokio::test]
async fn first_page_chain_uses_one_deadline_across_initial_and_continuation_requests() {
    let started = Instant::now();
    let result = first_page_with_deadline(
        Duration::from_millis(150),
        async {
            sleep(Duration::from_millis(30)).await;
            Ok::<_, String>(page("first", Some("A")))
        },
        10,
        |token, remaining| async move {
            assert!(remaining > Duration::ZERO);
            match token.as_str() {
                "A" => {
                    sleep(Duration::from_millis(30)).await;
                    Ok(page("second", Some("B")))
                }
                "B" => std::future::pending::<Result<ParsedPage, String>>().await,
                other => panic!("unexpected continuation token: {other}"),
            }
        },
    )
    .await
    .expect("later continuation timeout should keep collected first-page items");

    assert_eq!(result.items.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
    assert_eq!(result.next_token.as_deref(), Some("B"));
    assert!(result.warning.as_deref().is_some_and(|warning| warning.contains("timed out")));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn first_request_timeout_returns_error_within_the_budget() {
    let started = Instant::now();
    let result = first_page_with_deadline(
        Duration::from_millis(25),
        std::future::pending::<Result<ParsedPage, String>>(),
        10,
        |_token, _remaining| async { Ok(page("unused", None)) },
    )
    .await;

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_millis(500));
}
