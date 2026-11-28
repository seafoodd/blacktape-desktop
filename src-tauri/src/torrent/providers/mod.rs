use super::super::inspect::{magnet_info_hash, normalize_info_hash};
use super::{
    clean_field, merge_hits, ProviderError, ProviderReport, Query, SearchHit,
    MAX_RESULT_TITLE_BYTES,
};
use reqwest::{header, Client, RequestBuilder, StatusCode};
use serde_json::Value;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::time::{sleep_until, timeout_at, Instant};
use url::Url;

pub(super) mod apibay;
pub(super) mod archive;
pub(super) mod knaben;
mod rutor;
pub(super) mod solid_torrents;

const MAX_ROWS: usize = 30;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_INTERVAL: Duration = Duration::from_millis(475);
const PROVIDER_BUDGET: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Provider {
    Archive,
    Knaben,
    ApiBay,
    SolidTorrents,
    Rutor,
    RuTracker,
}

impl Provider {
    pub(in crate::torrent) const ALL: [Self; 5] = [Self::Archive, Self::Knaben, Self::ApiBay, Self::SolidTorrents, Self::Rutor];

    pub(in crate::torrent) fn name(self) -> &'static str {
        match self {
            Self::Archive => "Archive.org",
            Self::Knaben => "Knaben",
            Self::ApiBay => "ApiBay",
            Self::SolidTorrents => "SolidTorrents",
            Self::Rutor => "RuTor",
            Self::RuTracker => "RuTracker",
        }
    }

    pub(super) async fn search(self, client: &Client, query: &Query) -> ProviderReport {
        static ARCHIVE: Gate = Gate::new();
        static KNABEN: Gate = Gate::new();
        static APIBAY: Gate = Gate::new();
        static SOLID_TORRENTS: Gate = Gate::new();
        static RUTOR: Gate = Gate::new();

        let gate = match self {
            Self::Archive => &ARCHIVE,
            Self::Knaben => &KNABEN,
            Self::ApiBay => &APIBAY,
            Self::SolidTorrents => &SOLID_TORRENTS,
            Self::Rutor => &RUTOR,
            Self::RuTracker => return ProviderReport::from_hits(self.name().into(), Vec::new())
                .with_error(ProviderError::Unavailable),
        };
        let deadline = Instant::now() + PROVIDER_BUDGET;
        let request = |title_only| match self {
            Self::Archive => archive::search_request(client, query, title_only),
            Self::Knaben => knaben::request(client, query, title_only),
            Self::ApiBay => apibay::request(client, query, title_only),
            Self::SolidTorrents => solid_torrents::request(client, query, title_only),
            Self::Rutor => rutor::request(client, query, title_only),
            Self::RuTracker => unreachable!("RuTracker uses the signed-in browser"),
        };
        let parse = |root| match self {
            Self::Archive => archive::parse_search(&root, query),
            Self::Knaben => knaben::parse(&root, query),
            Self::ApiBay => apibay::parse(&root, query),
            Self::SolidTorrents => solid_torrents::parse(&root, query),
            Self::Rutor => unreachable!("RuTor returns HTML"),
            Self::RuTracker => unreachable!("RuTracker uses the signed-in browser"),
        };
        let fetch = |title_only| async move {
            if self == Self::Rutor {
                let bytes = gate.body(request(title_only), deadline).await?;
                let html = std::str::from_utf8(&bytes).map_err(|_| ProviderError::Malformed)?;
                return rutor::parse(html, query);
            }
            gate.json(request(title_only), deadline).await.and_then(parse)
        };
        let first = fetch(false).await;
        let (mut hits, mut error) = match first {
            Ok(hits) => (hits, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        if should_search_title_only(query, &hits, error) {
            match fetch(true).await {
                Ok(additional) => {
                    hits.extend(additional);
                    hits = merge_hits(hits);
                }
                Err(failure) => error = Some(failure),
            }
        }
        if matches!(self, Self::Archive) && !hits.is_empty() {
            let (verified, verification_error) = archive::check_hits(client, gate, deadline, hits).await;
            hits = verified;
            error = error.or(verification_error);
        }
        let report = ProviderReport::from_hits(self.name().into(), hits);
        match error {
            Some(error) => report.with_error(error),
            None => report,
        }
    }
}

fn should_search_title_only(query: &Query, hits: &[SearchHit], error: Option<ProviderError>) -> bool {
    query.can_search_title_only() && error.is_none() && !hits.iter().any(|hit| {
        hit.rank >= 2 && (hit.provider == "Archive.org" || hit.seeders.is_some_and(|count| count > 0))
    })
}

pub(super) struct Gate(Mutex<Option<(Instant, Option<ProviderError>)>>);

impl Gate {
    pub(super) const fn new() -> Self {
        Self(Mutex::const_new(None))
    }

    pub(super) async fn json(
        &self,
        request: RequestBuilder,
        deadline: Instant,
    ) -> Result<Value, ProviderError> {
        let body = self.body(request, deadline).await?;
        serde_json::from_slice(&body).map_err(|_| ProviderError::Malformed)
    }

    async fn body(
        &self,
        request: RequestBuilder,
        deadline: Instant,
    ) -> Result<Vec<u8>, ProviderError> {
        let mut slot = timeout_at(deadline, self.0.lock())
            .await
            .map_err(|_| ProviderError::Timeout)?;
        let retry = request.try_clone();
        for request in std::iter::once(request).chain(retry) {
            if let Some((next_request, error)) = *slot {
                if next_request >= deadline {
                    return Err(error.unwrap_or(ProviderError::Timeout));
                }
                sleep_until(next_request).await;
            }
            if Instant::now() >= deadline {
                return Err(ProviderError::Timeout);
            }

            *slot = Some((Instant::now() + REQUEST_INTERVAL, None));
            let response = timeout_at(deadline, request.send())
                .await
                .map_err(|_| ProviderError::Timeout)?
                .map_err(transport_error)?;
            if response.status().is_success() {
                return timeout_at(deadline, bounded_body(response))
                    .await
                    .map_err(|_| ProviderError::Timeout)?;
            }

            let code = response.status();
            let backoff = retry_after(response.headers());
            let cooldown = match (code, backoff) {
                (StatusCode::TOO_MANY_REQUESTS, backoff) => Some((
                    backoff.unwrap_or(Duration::from_secs(2)),
                    ProviderError::RateLimited,
                )),
                (StatusCode::SERVICE_UNAVAILABLE, Some(backoff)) => {
                    Some((backoff, ProviderError::Unavailable))
                }
                _ => None,
            };
            if let Some((backoff, error)) = cooldown {
                let backoff = backoff.clamp(REQUEST_INTERVAL, Duration::from_secs(31_536_000));
                *slot = Some((Instant::now() + backoff, Some(error)));
            } else {
                return Err(ProviderError::Unavailable);
            }
        }
        Err(slot
            .and_then(|(_, error)| error)
            .unwrap_or(ProviderError::Unavailable))
    }
}

fn retry_after(headers: &header::HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            let date = httpdate::parse_http_date(value).ok()?;
            Some(date.duration_since(SystemTime::now()).unwrap_or_default())
        })
}

fn transport_error(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else if error.is_body() || error.is_decode() {
        ProviderError::Malformed
    } else {
        ProviderError::Unavailable
    }
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, ProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ProviderError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(ProviderError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn count(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn title(row: &Value, key: &str) -> Option<String> {
    clean_field(row.get(key)?.as_str()?, MAX_RESULT_TITLE_BYTES)
}

fn parse_rows<'a>(
    rows: impl IntoIterator<Item = &'a Value>,
    parse: impl Fn(&Value) -> Option<(SearchHit, Option<u8>)>,
) -> Result<Vec<SearchHit>, ProviderError> {
    let mut found_row = false;
    let mut valid_row = false;
    let mut hits = Vec::new();
    for row in rows.into_iter().take(MAX_ROWS) {
        found_row = true;
        if let Some((mut hit, identity)) = parse(row) {
            valid_row = true;
            if let Some(rank) = identity {
                hit.rank = rank;
                hits.push(hit);
            }
        }
    }
    if found_row && !valid_row {
        return Err(ProviderError::Malformed);
    }
    Ok(merge_hits(hits))
}

fn magnet_hit(
    provider: Provider,
    title: String,
    hash: String,
    raw_magnet: Option<&str>,
    preferred_source: Option<Url>,
    seeders: Option<u64>,
    size_bytes: Option<u64>,
) -> SearchHit {
    let provider = provider.name().to_string();
    SearchHit {
        id: format!("btih:{hash}"),
        title,
        input: magnet_input(&hash, raw_magnet, preferred_source),
        providers: vec![provider.clone()],
        provider,
        info_hash: Some(hash),
        seeders,
        size_bytes,
        popularity: None,
        rank: 0,
    }
}

fn magnet_input(hash: &str, raw_magnet: Option<&str>, preferred_source: Option<Url>) -> String {
    let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
    let mut exact_sources = preferred_source
        .into_iter()
        .map(|url| url.to_string())
        .collect::<Vec<_>>();
    if let Some(raw) = raw_magnet.and_then(|raw| Url::parse(raw).ok()) {
        let mut sources = raw
            .query_pairs()
            .filter_map(|(key, value)| {
                if key != "xs" || value.len() > 1024 {
                    return None;
                }
                let source = Url::parse(&value).ok()?;
                (source.scheme() == "https"
                    && source.host().is_some()
                    && source.username().is_empty()
                    && source.password().is_none()
                    && source.query().is_none()
                    && source.fragment().is_none())
                .then(|| source.to_string())
            })
            .take(8)
            .collect::<Vec<_>>();
        sources.sort_unstable();
        sources.dedup();
        for source in sources {
            if !exact_sources.contains(&source) {
                exact_sources.push(source);
            }
        }
    }
    for source in exact_sources.into_iter().take(2) {
        magnet.push_str("&xs=");
        magnet.extend(url::form_urlencoded::byte_serialize(source.as_bytes()));
    }
    magnet
}
