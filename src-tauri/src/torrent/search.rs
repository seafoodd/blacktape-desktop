use crate::search::ranking::normalize;
use reqwest::Client;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::task::JoinSet;

#[path = "providers/mod.rs"]
mod providers;

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;

const MAX_QUERY_FIELD_BYTES: usize = 512;
const MAX_RESULT_TITLE_BYTES: usize = 1536;
pub(crate) use providers::Provider;

#[derive(Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub input: String,
    pub provider: String,
    pub providers: Vec<String>,
    pub info_hash: Option<String>,
    pub seeders: Option<u64>,
    pub size_bytes: Option<u64>,
    pub popularity: Option<u64>,
    pub rank: u8,
}

#[derive(Clone, Serialize)]
pub struct ProviderReport {
    pub provider: String,
    pub hits: Vec<SearchHit>,
    pub error: Option<String>,
    pub error_kind: Option<ProviderError>,
}

impl ProviderReport {
    fn from_hits(provider: String, hits: Vec<SearchHit>) -> Self {
        Self {
            provider,
            hits,
            error: None,
            error_kind: None,
        }
    }

    fn with_error(mut self, error: ProviderError) -> Self {
        self.error = Some(error.label().to_string());
        self.error_kind = Some(error);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    Malformed,
    Timeout,
    RateLimited,
    Unavailable,
    ResponseTooLarge,
}

impl ProviderError {
    fn label(self) -> &'static str {
        match self {
            Self::Malformed => "Invalid response from index",
            Self::Timeout => "Index request timed out",
            Self::RateLimited => "Index is rate limiting requests",
            Self::Unavailable => "Index unavailable",
            Self::ResponseTooLarge => "Index response exceeded the size limit",
        }
    }
}

#[derive(Serialize)]
pub struct SearchReport {
    pub providers: Vec<ProviderReport>,
    pub hits: Vec<SearchHit>,
}

#[derive(Clone)]
struct Query {
    artist: String,
    title: String,
    artist_words: Vec<String>,
    title_words: Vec<String>,
    verify_tracklist: bool,
}

impl Query {
    fn new(artist: &str, title: &str) -> Result<Self, String> {
        let artist = clean_field(artist, MAX_QUERY_FIELD_BYTES)
            .ok_or_else(|| "Enter an artist for torrent discovery.".to_string())?;
        let title = clean_field(title, MAX_QUERY_FIELD_BYTES)
            .ok_or_else(|| "Enter a release or track title for torrent discovery.".to_string())?;
        let title = match (normalize(&artist).as_str(), normalize(&title).as_str()) {
            ("car seat headrest", "living while starving") => "Starving While Living".into(),
            _ => title,
        };
        let artist_words = normalize(&artist)
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        let title_words = normalize(&title)
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        if artist_words.is_empty() || title_words.is_empty() {
            return Err(
                "Torrent discovery needs an artist and title with letters or numbers.".into(),
            );
        }
        Ok(Self {
            artist,
            title,
            artist_words,
            title_words,
            verify_tracklist: false,
        })
    }

    fn text(&self, title_only: bool) -> String {
        if title_only {
            self.title.clone()
        } else {
            format!("{} {}", self.artist, self.title)
        }
    }

    fn can_search_title_only(&self) -> bool {
        self.verify_tracklist && self.title.chars().filter(|c| c.is_alphabetic()).take(3).count() == 3
    }

    fn artist_prefix<'a>(&self, words: &'a [&str]) -> Option<&'a [&'a str]> {
        let (prefix, rest) = words.split_at_checked(self.artist_words.len())?;
        prefix
            .iter()
            .copied()
            .eq(self.artist_words.iter().map(String::as_str))
            .then_some(rest)
    }

    fn matches_creator(&self, creator: &str) -> bool {
        normalize(creator)
            .split_whitespace()
            .eq(self.artist_words.iter().map(String::as_str))
    }

    fn identity(&self, raw_title: &str, creators: &[&str]) -> Option<u8> {
        let normalized = normalize(raw_title);
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        let leading = words.iter().take_while(|word| is_format_word(word) || is_year(word)).count();
        let creator_matches = creators.iter().any(|creator| self.matches_creator(creator));
        let title: Vec<_> = self.title_words.iter().map(String::as_str).collect();
        let artist: Vec<_> = self.artist_words.iter().map(String::as_str).collect();
        [words.as_slice(), &words[leading..]].into_iter().filter_map(|words| {
            // Some indexes place the artist after the title, including soundtrack and vinyl catalogues.
            let reversed = words.strip_prefix(title.as_slice()).and_then(|tail| {
                tail.windows(artist.len()).enumerate().filter(|(_, part)| *part == artist.as_slice())
                    .find_map(|(start, _)| {
                        let front = suffix_rank(&tail[..start])?;
                        let back = suffix_rank(&tail[start + artist.len()..])?;
                        Some(front.min(back))
                    })
            });
            self.artist_prefix(words)
                .and_then(|tail| self.title_rank(tail, true))
                .or(reversed)
                .or_else(|| creator_matches.then(|| self.title_rank(words, true)).flatten())
                .or_else(|| {
                    (creators.is_empty() && self.can_search_title_only())
                        .then(|| self.title_rank(words, false).map(|_| 1)).flatten()
                })
        }).max()
    }

    fn title_rank(&self, words: &[&str], artist_verified: bool) -> Option<u8> {
        let title = self
            .title_words
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let after_year = words
            .split_first()
            .and_then(|(first, rest)| is_year(first).then_some(rest));
        std::iter::once(words)
            .chain(after_year)
            .filter_map(|candidate| candidate.strip_prefix(title.as_slice()))
            .find_map(|suffix| suffix_rank(suffix).or_else(|| {
                (artist_verified && self.can_search_title_only() && suffix.len() <= 6
                    && !suffix.iter().any(|word| matches!(*word,
                        "live" | "remix" | "remixed" | "mix" | "mixes" | "demo" | "demos" | "stems"
                        | "instrumental" | "instrumentals" | "karaoke" | "cover" | "covers" | "tribute"
                        | "acoustic" | "rerecorded" | "version" | "versions" | "unplugged" | "bootleg"
                        | "discography" | "collection" | "concert")))
                    .then_some(1)
            }))
    }
}

fn clean_field(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes {
        return None;
    }
    let cleaned = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!cleaned.chars().any(char::is_control)).then_some(cleaned)
}

fn is_year(word: &str) -> bool {
    word.len() == 4
        && word
            .parse::<u16>()
            .is_ok_and(|year| (1900..=2099).contains(&year))
}

fn is_format_word(word: &str) -> bool {
    matches!(
        word,
        "flac"
            | "mp3"
            | "aac"
            | "m4a"
            | "alac"
            | "ape"
            | "wav"
            | "wv"
            | "opus"
            | "ogg"
            | "lossless"
            | "web"
            | "webdl"
            | "vinyl"
            | "rip"
            | "eac"
            | "log"
            | "cue"
            | "v0"
            | "v2"
            | "cbr"
            | "vbr"
            | "kbps"
            | "khz"
            | "bit"
            | "16bit"
            | "24bit"
            | "32bit"
            | "320kbps"
            | "44khz"
            | "48khz"
            | "96khz"
            | "192khz"
            | "hires"
            | "stereo"
            | "mono"
    )
}

fn is_ordinal(word: &str) -> bool {
    ["st", "nd", "rd", "th"].iter().any(|suffix| {
        word.strip_suffix(suffix)
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn suffix_rank(suffix: &[&str]) -> Option<u8> {
    let suffix = suffix.strip_prefix(&["full", "album"]).unwrap_or(suffix);
    let mut rank = 3;
    for (index, word) in suffix.iter().enumerate() {
        match *word {
            "deluxe" | "expanded" | "remaster" | "remastered" | "edition" | "reissue"
            | "anniversary" | "bonus" | "tracks" | "disc" | "disk" | "cd" | "cd1" | "cd2"
            | "cd3" => rank = 2,
            "ost" | "soundtrack" => rank = 2,
            "original" | "game" | "video" | "motion" | "picture"
                if suffix.iter().any(|word| matches!(*word, "ost" | "soundtrack")) => rank = 2,
            word if is_ordinal(word)
                && suffix.get(index + 1).is_some_and(|next| matches!(*next, "anniversary" | "ann")) => rank = 2,
            "ann" if index > 0 && is_ordinal(suffix[index - 1]) => rank = 2,
            word if is_format_word(word) || is_year(word) => {}
            word if word.bytes().all(|byte| byte.is_ascii_digit())
                && index > 0 && matches!(suffix[index - 1], "cd" | "disc" | "disk") => {}
            "16" | "24" | "32" | "44" | "48" | "88" | "96" | "128" | "176" | "192" | "256" | "320" | "384"
                if suffix.iter().any(|word| is_format_word(word)) => {}
            "1" | "2" | "4" if index > 0 && matches!(suffix[index - 1], "44" | "88" | "176")
                && suffix.iter().any(|word| is_format_word(word)) => {}
            _ => return None,
        }
    }
    Some(rank)
}

fn sort_hits(hits: &mut [SearchHit]) {
    hits.sort_by(|left, right| {
        right
            .rank
            .cmp(&left.rank)
            .then_with(|| right.seeders.cmp(&left.seeders))
            .then_with(|| right.popularity.cmp(&left.popularity))
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.input.cmp(&right.input))
            .then_with(|| left.size_bytes.cmp(&right.size_bytes))
    });
}

pub(super) fn merge_hits(mut hits: Vec<SearchHit>) -> Vec<SearchHit> {
    sort_hits(&mut hits);
    let mut unique = BTreeMap::<String, SearchHit>::new();
    for hit in hits {
        if let Some(existing) = unique.get_mut(&hit.id) {
            existing.providers.extend(hit.providers);
            existing.providers.sort_unstable();
            existing.providers.dedup();
            existing.seeders = existing.seeders.max(hit.seeders);
            existing.size_bytes = existing.size_bytes.or(hit.size_bytes);
            if !existing.input.contains("&xs=") && hit.input.contains("&xs=") {
                existing.input = hit.input;
            }
        } else {
            unique.insert(hit.id.clone(), hit);
        }
    }
    let mut hits = unique.into_values().collect::<Vec<_>>();
    sort_hits(&mut hits);
    hits
}

async fn collect(
    mut jobs: JoinSet<ProviderReport>,
    progress: impl Fn(ProviderReport),
) -> SearchReport {
    let mut providers = Vec::new();
    while let Some(result) = jobs.join_next().await {
        if let Ok(report) = result {
            progress(report.clone());
            providers.push(report);
        }
    }
    providers.sort_by(|left, right| left.provider.cmp(&right.provider));
    let hits = merge_hits(
        providers
            .iter()
            .flat_map(|provider| provider.hits.iter().cloned())
            .collect(),
    );
    SearchReport { providers, hits }
}

#[cfg(test)]
pub async fn search(
    artist: &str,
    title: &str,
    progress: impl Fn(ProviderReport),
) -> Result<SearchReport, String> {
    search_for_release(artist, title, false, &Provider::ALL, progress).await
}

fn search_client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, reqwest::Error>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .user_agent(concat!("Blacktape/", env!("CARGO_PKG_VERSION")))
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true)
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(7))
                .build()
        })
        .as_ref()
        .map_err(|_| "Could not initialize torrent search.".to_string())
}

#[cfg(test)]
pub(super) async fn search_for_release(
    artist: &str, title: &str, verify_tracklist: bool, enabled: &[Provider], progress: impl Fn(ProviderReport),
) -> Result<SearchReport, String> {
    search_with_app(None, artist, title, verify_tracklist, enabled, progress).await
}

pub(super) async fn search_with_app(
    app: Option<&tauri::AppHandle>,
    artist: &str,
    title: &str,
    verify_tracklist: bool,
    enabled: &[Provider],
    progress: impl Fn(ProviderReport),
) -> Result<SearchReport, String> {
    let mut query = Query::new(artist, title)?;
    query.verify_tracklist = verify_tracklist;
    let client = search_client()?;
    let mut jobs = JoinSet::new();
    for &provider in enabled {
        let query = query.clone();
        #[cfg(not(test))]
        if let (Provider::RuTracker, Some(app)) = (provider, app) {
            let app = app.clone();
            jobs.spawn(async move { search_rutracker(&app, &query).await });
            continue;
        }
        #[cfg(test)]
        let _ = app;
        jobs.spawn(async move { provider.search(client, &query).await });
    }
    Ok(collect(jobs, progress).await)
}

pub(super) async fn test_provider(provider: Provider, app: Option<&tauri::AppHandle>) -> Result<ProviderReport, String> {
    let query = Query::new("The Beatles", "Abbey Road")?;
    if let (Provider::RuTracker, Some(app)) = (provider, app) {
        return Ok(search_rutracker(app, &query).await);
    }
    Ok(provider.search(search_client()?, &query).await)
}

async fn search_rutracker(app: &tauri::AppHandle, query: &Query) -> ProviderReport {
    use super::rutracker::{self, Failure};
    let provider = Provider::RuTracker.name().to_owned();
    let mut report = ProviderReport::from_hits(provider, Vec::new());
    let rows = match rutracker::search(app, &query.text(false)).await {
        Ok(rows) => rows,
        Err(error) => {
            report.error_kind = Some(match error {
                Failure::RateLimited => ProviderError::RateLimited,
                Failure::Timeout => ProviderError::Timeout,
                Failure::Malformed => ProviderError::Malformed,
                Failure::ResponseTooLarge => ProviderError::ResponseTooLarge,
                _ => ProviderError::Unavailable,
            });
            report.error = Some(error.message().into());
            return report;
        }
    };
    report.hits = rutracker_hits(query, rows);
    report
}

fn rutracker_hits(query: &Query, rows: Vec<super::rutracker::Row>) -> Vec<SearchHit> {
    let provider = Provider::RuTracker.name().to_owned();
    let hits = rows.into_iter().take(100).filter_map(|row| {
        let topic = super::rutracker::topic_from_link(&row.link)?;
        let title = clean_field(&row.title, MAX_RESULT_TITLE_BYTES)?;
        // RuTracker commonly prefixes titles with bracketed format and parenthesized genre tags.
        let candidate = title.trim_start();
        let candidate = trim_release_prefixes(candidate);
        let rank = query.identity(candidate, &[])?;
        Some(SearchHit {
            id: format!("rutracker:{topic}"), title, input: format!("rutracker:{topic}"),
            provider: provider.clone(), providers: vec![provider.clone()], info_hash: None,
            seeders: row.seeders, size_bytes: None, popularity: None, rank,
        })
    }).collect();
    merge_hits(hits).into_iter().take(30).collect()
}

fn trim_release_prefixes(mut title: &str) -> &str {
    while let Some(close) = match title.as_bytes().first() {
        Some(b'[') => title.find(']'),
        Some(b'(') => title.find(')'),
        _ => None,
    } {
        let rest = title[close + 1..].trim_start();
        if rest.is_empty() { break; }
        title = rest;
    }
    title
}
