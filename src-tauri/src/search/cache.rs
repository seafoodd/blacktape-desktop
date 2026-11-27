use super::{progress::Source, SearchSuggestion, SourceResults};
use crate::library_index::sources::TtlCache;
use std::future::Future;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

fn cache() -> &'static Mutex<TtlCache<Vec<SearchSuggestion>>> {
    static CACHE: OnceLock<Mutex<TtlCache<Vec<SearchSuggestion>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TtlCache::new(Duration::from_secs(5 * 60))))
}

pub async fn cached_source(
    cache: &Mutex<TtlCache<Vec<SearchSuggestion>>>,
    source: Source,
    query: &str,
    now: Instant,
    request: impl Future<Output = Result<SourceResults, String>>,
) -> Result<SourceResults, String> {
    let key = format!("{source:?}:{}", query.trim().to_lowercase());
    if let Some(items) = cache.lock().unwrap_or_else(|error| error.into_inner()).get(&key, now) {
        return Ok(SourceResults { items, complete: true });
    }
    let result = request.await?;
    if result.complete {
        cache.lock().unwrap_or_else(|error| error.into_inner()).put(key, result.items.clone(), now);
    }
    Ok(result)
}

pub async fn source_results(
    source: Source,
    query: &str,
    report: &super::progress::Report,
    request: impl Future<Output = Result<SourceResults, String>>,
) -> Result<Vec<SearchSuggestion>, String> {
    let request = cached_source(cache(), source, query, Instant::now(), request);
    super::progress::track_checked(report, source, request, |result| result.items.len(), |result| result.complete)
        .await.map(|result| result.items)
}
