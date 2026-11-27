use super::{dropdown_deadline, page_deadline, with_deadline, Source, State, track};
use std::{sync::{Arc, Mutex}, time::Duration};

#[test]
fn source_deadlines_match_dropdown_and_page_budgets() {
    assert_eq!(dropdown_deadline(Source::Youtube), Duration::from_secs(9));
    assert_eq!(dropdown_deadline(Source::Bandcamp), Duration::from_secs(6));
    assert_eq!(dropdown_deadline(Source::Soundcloud), Duration::from_secs(10));
    assert_eq!(dropdown_deadline(Source::Musicbrainz), Duration::from_secs(6));

    assert_eq!(page_deadline(Source::Youtube), Duration::from_secs(12));
    assert_eq!(page_deadline(Source::Bandcamp), Duration::from_secs(6));
    assert_eq!(page_deadline(Source::Soundcloud), Duration::from_secs(10));
    assert_eq!(page_deadline(Source::Musicbrainz), Duration::from_secs(6));
}

#[tokio::test]
async fn deadline_failure_is_reported_as_running_then_failed() {
    let states = Arc::new(Mutex::new(Vec::new()));
    let states_for_report = Arc::clone(&states);
    let report: super::Report = Arc::new(move |progress: super::SourceProgress| {
        states_for_report.lock().unwrap().push(progress.state);
    });

    let result = track(
        &report,
        Source::Youtube,
        with_deadline(Duration::from_millis(5), std::future::pending::<Result<Vec<i32>, String>>()),
        Vec::len,
    )
    .await;

    assert!(result.unwrap_err().contains("timed out"));
    assert_eq!(*states.lock().unwrap(), vec![State::Running, State::Failed]);
}
