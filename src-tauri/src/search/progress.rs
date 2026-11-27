//! What each search source is doing, reported while a search runs so the UI can show it.
//! Mirrored in `src/shared/lib/search/searchProgress.ts`.

use serde::Serialize;
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Youtube,
    Bandcamp,
    Soundcloud,
    Musicbrainz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Will be asked, but not yet.
    Waiting,
    Running,
    Done,
    Failed,
}

/// Carries no error text: request errors quote their URL, and some URLs hold keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProgress {
    pub source: Source,
    pub state: State,
    /// How long the source took to answer or fail.
    pub elapsed_ms: Option<u64>,
    /// Results it returned.
    pub count: Option<usize>,
}

pub type Report = Arc<dyn Fn(SourceProgress) + Send + Sync>;

pub fn ignore() -> Report {
    Arc::new(|_| {})
}

pub fn waiting(report: &Report, source: Source) {
    report(SourceProgress { source, state: State::Waiting, elapsed_ms: None, count: None });
}

/// Runs the request of one source, reporting when it starts and how it ends. Returns what it returned.
pub async fn track<T, E>(
    report: &Report,
    source: Source,
    request: impl Future<Output = Result<T, E>>,
    count: impl FnOnce(&T) -> usize,
) -> Result<T, E> {
    report(SourceProgress { source, state: State::Running, elapsed_ms: None, count: None });
    let started = Instant::now();
    let result = request.await;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (state, count) = match &result {
        Ok(value) => (State::Done, Some(count(value))),
        Err(_) => (State::Failed, None),
    };
    report(SourceProgress { source, state, elapsed_ms: Some(elapsed_ms), count });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    type Seen = Arc<Mutex<Vec<SourceProgress>>>;

    fn recorder() -> (Report, Seen) {
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let report: Report = Arc::new(move |progress: SourceProgress| {
            sink.lock().expect("progress lock").push(progress);
        });
        (report, seen)
    }

    fn reported(seen: &Seen) -> Vec<SourceProgress> {
        seen.lock().expect("progress lock").clone()
    }

    #[tokio::test]
    async fn track_reports_running_then_done_with_count_and_returns_the_value() {
        let (report, seen) = recorder();

        let value = track(
            &report,
            Source::Youtube,
            async { Ok::<Vec<i32>, ()>(vec![1, 2, 3]) },
            |v: &Vec<i32>| v.len(),
        )
        .await;

        assert_eq!(value, Ok(vec![1, 2, 3]));
        let seen = reported(&seen);
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[0],
            SourceProgress {
                source: Source::Youtube,
                state: State::Running,
                elapsed_ms: None,
                count: None,
            }
        );
        assert_eq!(seen[1].source, Source::Youtube);
        assert_eq!(seen[1].state, State::Done);
        assert_eq!(seen[1].count, Some(3));
        assert!(seen[1].elapsed_ms.is_some());
    }

    #[tokio::test]
    async fn track_reports_running_then_failed_with_elapsed_and_no_count() {
        let (report, seen) = recorder();

        let result = track(
            &report,
            Source::Bandcamp,
            async { Err::<Vec<i32>, &str>("timeout") },
            |v: &Vec<i32>| v.len(),
        )
        .await;

        assert_eq!(result, Err("timeout"));
        let seen = reported(&seen);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].state, State::Running);
        assert_eq!(seen[1].source, Source::Bandcamp);
        assert_eq!(seen[1].state, State::Failed);
        assert!(seen[1].elapsed_ms.is_some());
        assert_eq!(seen[1].count, None);
    }

    #[tokio::test]
    async fn elapsed_ms_covers_the_time_of_the_request() {
        let (report, seen) = recorder();

        let _ = track(
            &report,
            Source::Soundcloud,
            async {
                std::thread::sleep(std::time::Duration::from_millis(30));
                Ok::<(), ()>(())
            },
            |_: &()| 0,
        )
        .await;

        let elapsed = reported(&seen)[1].elapsed_ms.expect("elapsed is set");
        assert!(elapsed >= 25, "elapsed was {elapsed} ms");
    }

    #[test]
    fn waiting_reports_waiting_without_timing() {
        let (report, seen) = recorder();

        waiting(&report, Source::Musicbrainz);

        assert_eq!(
            reported(&seen),
            vec![SourceProgress {
                source: Source::Musicbrainz,
                state: State::Waiting,
                elapsed_ms: None,
                count: None,
            }]
        );
    }

    #[test]
    fn serializes_to_the_shape_the_frontend_reads() {
        let progress = SourceProgress {
            source: Source::Musicbrainz,
            state: State::Done,
            elapsed_ms: Some(12),
            count: Some(3),
        };

        let json = serde_json::to_value(&progress).expect("serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "source": "musicbrainz",
                "state": "done",
                "elapsedMs": 12,
                "count": 3
            })
        );
    }
}
