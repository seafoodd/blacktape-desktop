#[test]
fn corpus_has_ten_distinct_identifiable_cases_per_group() {
    let cases = read_corpus();
    assert_eq!(cases.len(), 80);
    let mut groups = std::collections::BTreeMap::new();
    let mut identities = std::collections::BTreeSet::new();
    for case in &cases {
        *groups.entry((case.kind.as_str(), case.group)).or_insert(0) += 1;
        assert!(identities.insert((case.kind.as_str(), case.artist, case.title)));
        assert!(case.reference.starts_with("https://"));
        assert!(!case.acceptable.is_empty() && !case.wrong_match.is_empty());
    }
    assert_eq!(groups.len(), 8);
    assert!(groups.into_values().all(|count| count == 10));
}

#[test]
fn real_catalog_near_matches_remain_distinct() {
    let cases = read_corpus();
    let case = |id| cases.iter().find(|case| case.id == id).unwrap();
    assert_eq!(
        case("A09").judge_title("Björk - Vespertine"),
        Judgment::Correct
    );
    assert_eq!(
        case("A09").judge_title("Björk - Vespertine Live"),
        Judgment::Wrong
    );
    assert_eq!(
        case("A11").judge_title("The Beatles - 1967-1970 1 CD"),
        Judgment::Wrong
    );
    assert_eq!(
        case("A21").judge_title("Taylor Swift - 1989 (2014)"),
        Judgment::Wrong
    );
    assert_eq!(
        case("A21").judge_title("Taylor Swift - 1989 Taylor's Version"),
        Judgment::Correct
    );
    assert_eq!(case("T11").judge_title("Metallica - One"), Judgment::Wrong);
    assert_eq!(
        case("T19").judge_title("Kendrick Lamar - untitled unmastered"),
        Judgment::Wrong
    );
    assert_eq!(
        case("T23").judge_title("Radiohead - A Moon Shaped Pool"),
        Judgment::Wrong
    );
}

#[test]
fn inspected_track_needs_the_right_file_and_recording() {
    let cases = read_corpus();
    let find = |id| cases.iter().find(|case| case.id == id).unwrap();
    let live = find("T23");
    assert_eq!(
        live.judge_files(
            "Radiohead - I Might Be Wrong (Live)",
            false,
            [(6, "True Love Waits")]
        ),
        (Judgment::Correct, Some(6))
    );
    assert_eq!(
        live.judge_files(
            "Radiohead - True Love Waits",
            false,
            [(3, "True Love Waits")]
        ),
        (Judgment::Ambiguous, None)
    );
    assert_eq!(
        live.judge_files(
            "Radiohead - I Might Be Wrong (Live)",
            false,
            [(6, "Idioteque")]
        ),
        (Judgment::Wrong, None)
    );
    assert_eq!(
        live.judge_files(
            "Radiohead - I Might Be Wrong (Live)",
            true,
            [(0, "True Love Waits")]
        ),
        (Judgment::Ambiguous, None)
    );
    assert_eq!(
        live.judge_files(
            "Radiohead - I Might Be Wrong (Live)",
            false,
            [
                (2, "True Love Waits"),
                (6, "True Love Waits (Live in Oslo)")
            ]
        ),
        (Judgment::Ambiguous, None)
    );
}

#[test]
fn checkpoint_report_renders_tables_and_pending_cases() {
    let report = report_markdown("2026-11-25", &read_corpus(), &BTreeMap::new());
    assert!(report.contains("\n## Results\n\n| Scope |"));
    assert!(report.contains("\n## Per-query results\n\n| ID |"));
    assert_eq!(
        report
            .lines()
            .filter(|line| line.contains("| Pending |"))
            .count(),
        80
    );
}

use super::{inspect, search};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

const CORPUS: &str = include_str!("evaluation-corpus.tsv");
const ATTEMPT_BUDGET: usize = 3;
const CONCURRENT_CASES: usize = 2;
const CASE_INTERVAL: Duration = Duration::from_secs(2);
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Album,
    Track,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Album => "album",
            Self::Track => "track",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VersionEvidence {
    Standard,
    Parent,
    Explicit,
}

#[derive(Clone)]
struct Case {
    id: &'static str,
    kind: Kind,
    group: &'static str,
    artist: &'static str,
    title: &'static str,
    reference: &'static str,
    artists: Vec<String>,
    releases: Vec<String>,
    file_titles: Vec<String>,
    exclusions: Vec<String>,
    acceptable: &'static str,
    wrong_match: &'static str,
    version: VersionEvidence,
}

fn read_corpus() -> Vec<Case> {
    CORPUS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            let [id, kind, group, artist, title, reference, aliases, parent, parent_aliases, files, exclusions, acceptable, wrong_match, version] =
                columns.as_slice()
            else {
                panic!("Malformed torrent evaluation corpus row")
            };
            Case {
                id,
                kind: match *kind {
                    "album" => Kind::Album,
                    "track" => Kind::Track,
                    _ => panic!("Unknown torrent evaluation query kind"),
                },
                group,
                artist,
                title,
                reference,
                artists: std::iter::once(*artist)
                    .chain(aliases.split('|'))
                    .filter(|name| !name.is_empty())
                    .map(normalize)
                    .collect(),
                releases: std::iter::once(*parent)
                    .chain(parent_aliases.split('|'))
                    .filter(|name| !name.is_empty())
                    .map(normalize)
                    .collect(),
                file_titles: files.split('|').map(normalize).collect(),
                exclusions: exclusions
                    .split('|')
                    .filter(|marker| !marker.is_empty())
                    .map(normalize)
                    .collect(),
                acceptable,
                wrong_match,
                version: match *version {
                    "" => VersionEvidence::Standard,
                    "parent" => VersionEvidence::Parent,
                    "explicit" => VersionEvidence::Explicit,
                    _ => panic!("Unknown torrent evaluation recording evidence rule"),
                },
            }
        })
        .collect()
}

fn normalize(value: &str) -> String {
    let mut normalized = String::new();
    for character in value
        .nfkd()
        .filter(|character| !is_combining_mark(*character))
    {
        if character.is_alphanumeric() {
            normalized.extend(character.to_lowercase());
        } else if character != '\'' && character != '’' && !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    normalized.trim().to_owned()
}

fn has_phrase(text: &str, phrase: &str) -> bool {
    !phrase.is_empty()
        && text.match_indices(phrase).any(|(start, matched)| {
            (start == 0 || text.as_bytes()[start - 1] == b' ')
                && (start + matched.len() == text.len()
                    || text.as_bytes()[start + matched.len()] == b' ')
        })
}

fn file_title_matches(full: &str, expected: &str) -> bool {
    let Some(suffix) = full.strip_prefix(expected) else {
        return false;
    };
    if suffix.is_empty() {
        return true;
    }
    let Some(suffix) = suffix.strip_prefix(' ') else {
        return false;
    };
    let mut mastering = false;
    for word in suffix.split(' ') {
        if matches!(word, "remaster" | "remastered") {
            mastering = true;
        } else if !word.bytes().all(|byte| byte.is_ascii_digit()) || word.len() != 4 {
            return false;
        }
    }
    mastering
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Judgment {
    Correct,
    Wrong,
    Ambiguous,
}

impl Case {
    fn mentions_artist(&self, title: &str) -> bool {
        self.artists.iter().any(|artist| has_phrase(title, artist))
    }

    fn names_work(&self, title: &str, work: &str) -> bool {
        self.artists.iter().any(|artist| {
            has_phrase(title, &format!("{artist} {work}"))
                || has_phrase(title, &format!("{work} {artist}"))
                || (work.chars().count() > 2
                    && !has_phrase(artist, work)
                    && has_phrase(title, artist)
                    && has_phrase(title, work))
        })
    }

    fn names_parent(&self, title: &str) -> bool {
        self.releases
            .iter()
            .any(|release| self.names_work(title, release))
    }

    fn excluded(&self, title: &str) -> bool {
        ["karaoke", "tribute", "discography"]
            .iter()
            .any(|marker| has_phrase(title, marker))
            || self
                .exclusions
                .iter()
                .any(|marker| has_phrase(title, marker))
    }

    fn wrong_standard_version(&self, title: &str) -> bool {
        [
            "live",
            "bootleg",
            "demo",
            "remix",
            "instrumental",
            "cover version",
            "re recorded",
        ]
        .iter()
        .any(|marker| has_phrase(title, marker))
    }

    fn judge_title(&self, headline: &str) -> Judgment {
        let title = normalize(headline);
        if self.excluded(&title)
            || (self.version == VersionEvidence::Standard && self.wrong_standard_version(&title))
        {
            return Judgment::Wrong;
        }
        if self.names_parent(&title) {
            return Judgment::Correct;
        }
        if self.kind == Kind::Track {
            let query = normalize(self.title);
            if self.version != VersionEvidence::Parent && self.names_work(&title, &query) {
                return Judgment::Correct;
            }
            if self
                .file_titles
                .iter()
                .any(|file| self.names_work(&title, file))
            {
                return if self.version == VersionEvidence::Standard {
                    Judgment::Correct
                } else {
                    Judgment::Ambiguous
                };
            }
        }
        if !self.mentions_artist(&title)
            && self
                .releases
                .iter()
                .any(|release| has_phrase(&title, release))
        {
            return Judgment::Ambiguous;
        }
        Judgment::Wrong
    }

    fn judge_files<'a>(
        &self,
        release_name: &str,
        cue_image: bool,
        files: impl IntoIterator<Item = (usize, &'a str)>,
    ) -> (Judgment, Option<usize>) {
        let release = normalize(release_name);
        let title_judgment = self.judge_title(release_name);
        if self.excluded(&release)
            || (self.version == VersionEvidence::Standard && self.wrong_standard_version(&release))
        {
            return (Judgment::Wrong, None);
        }
        if cue_image {
            return (Judgment::Ambiguous, None);
        }
        let files = files
            .into_iter()
            .map(|(index, title)| (index, normalize(title)))
            .collect::<Vec<_>>();
        if self.kind == Kind::Album {
            let witnesses_present = self.file_titles.iter().all(|expected| {
                files.iter().any(|(_, title)| {
                    file_title_matches(title, expected) || has_phrase(title, expected)
                })
            });
            if title_judgment == Judgment::Correct && witnesses_present {
                return (Judgment::Correct, None);
            }
            return (Judgment::Ambiguous, None);
        }

        let matched = files
            .iter()
            .filter(|(_, file)| {
                !self.excluded(file)
                    && self.file_titles.iter().any(|expected| {
                        file_title_matches(file, expected)
                            || self.artists.iter().any(|artist| {
                                file.strip_prefix(artist)
                                    .and_then(|tail| tail.strip_prefix(' '))
                                    .is_some_and(|tail| file_title_matches(tail, expected))
                            })
                    })
            })
            .map(|(index, _)| *index)
            .collect::<BTreeSet<_>>();
        match matched.len() {
            0 => (Judgment::Wrong, None),
            1 if title_judgment == Judgment::Correct => {
                (Judgment::Correct, matched.first().copied())
            }
            _ => (Judgment::Ambiguous, None),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Provider {
    Archive,
    Knaben,
    ApiBay,
    SolidTorrents,
    Rutor,
    Other,
}

impl Provider {
    const INDEXERS: [Self; 5] = [Self::Archive, Self::Knaben, Self::ApiBay, Self::SolidTorrents, Self::Rutor];

    fn parse(name: &str) -> Self {
        match normalize(name).as_str() {
            "archive org" | "archive" | "internet archive" => Self::Archive,
            "knaben" => Self::Knaben,
            "apibay" | "api bay" => Self::ApiBay,
            "solidtorrents" | "solid torrents" => Self::SolidTorrents,
            "rutor" => Self::Rutor,
            _ => Self::Other,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Archive => "Archive.org",
            Self::Knaben => "Knaben",
            Self::ApiBay => "ApiBay",
            Self::SolidTorrents => "SolidTorrents",
            Self::Rutor => "RuTor",
            Self::Other => "Unknown provider",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderStatus {
    Hits,
    Empty,
    RateLimited,
    Timeout,
    Malformed,
    Unavailable,
}

impl ProviderStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Hits => "hits",
            Self::Empty => "empty",
            Self::RateLimited => "rate limited",
            Self::Timeout => "timed out",
            Self::Malformed => "malformed",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ProviderOutcome {
    provider: Provider,
    status: ProviderStatus,
    hit_count: usize,
    top_titles: Vec<Judgment>,
}

impl ProviderOutcome {
    fn status(error: Option<&str>, hit_count: usize) -> ProviderStatus {
        let Some(error) = error.map(str::to_ascii_lowercase) else {
            return if hit_count == 0 {
                ProviderStatus::Empty
            } else {
                ProviderStatus::Hits
            };
        };
        if error.contains("rate limit") || error.contains("429") {
            ProviderStatus::RateLimited
        } else if error.contains("timed out") || error.contains("timeout") {
            ProviderStatus::Timeout
        } else if error.contains("malformed")
            || error.contains("invalid")
            || error.contains("too large")
        {
            ProviderStatus::Malformed
        } else {
            ProviderStatus::Unavailable
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum InputKind {
    Magnet,
    TorrentFile,
}

impl InputKind {
    fn label(self) -> &'static str {
        match self {
            Self::Magnet => "magnet",
            Self::TorrentFile => "torrent file",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum InspectionStatus {
    Verified,
    Wrong,
    Ambiguous,
    Timeout,
    Failed,
}

impl InspectionStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Wrong => "wrong",
            Self::Ambiguous => "ambiguous",
            Self::Timeout => "timed out",
            Self::Failed => "failed",
        }
    }
}

#[derive(Serialize, Deserialize)]
struct InspectionOutcome {
    rank: usize,
    provider: Provider,
    input: InputKind,
    status: InspectionStatus,
    elapsed_ms: u128,
    file_index: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct CaseOutcome {
    id: String,
    search_ms: u128,
    search_failed: bool,
    providers: Vec<ProviderOutcome>,
    hits: usize,
    top_titles: Vec<Judgment>,
    inspections: Vec<InspectionOutcome>,
}

impl CaseOutcome {
    fn verified(&self) -> bool {
        self.inspections
            .iter()
            .any(|attempt| attempt.status == InspectionStatus::Verified)
    }

    fn ambiguous(&self) -> bool {
        !self.verified()
            && (self.top_titles.contains(&Judgment::Ambiguous)
                || self
                    .inspections
                    .iter()
                    .any(|attempt| attempt.status == InspectionStatus::Ambiguous))
    }

    fn wrong_match(&self) -> bool {
        self.top_titles.contains(&Judgment::Wrong)
            || self
                .inspections
                .iter()
                .any(|attempt| attempt.status == InspectionStatus::Wrong)
    }
}

async fn evaluate(case: &Case) -> CaseOutcome {
    let search_started = Instant::now();
    let report = search::search(case.artist, case.title, |_| {}).await;
    let mut result = CaseOutcome {
        id: case.id.to_owned(),
        search_ms: search_started.elapsed().as_millis(),
        search_failed: report.is_err(),
        providers: Vec::new(),
        hits: 0,
        top_titles: Vec::new(),
        inspections: Vec::new(),
    };
    let Ok(report) = report else {
        return result;
    };
    for provider in report.providers {
        result.providers.push(ProviderOutcome {
            provider: Provider::parse(&provider.provider),
            status: ProviderOutcome::status(provider.error.as_deref(), provider.hits.len()),
            hit_count: provider.hits.len(),
            top_titles: provider
                .hits
                .iter()
                .take(ATTEMPT_BUDGET)
                .map(|hit| case.judge_title(&hit.title))
                .collect(),
        });
    }
    result.hits = report.hits.len();
    let mut seen = BTreeSet::new();
    for (offset, hit) in report
        .hits
        .iter()
        .filter(|hit| seen.insert(hit.id.as_str()))
        .take(ATTEMPT_BUDGET)
        .enumerate()
    {
        result.top_titles.push(case.judge_title(&hit.title));
        if result.verified() {
            continue;
        }
        let started = Instant::now();
        let inspected = inspect::inspect(&hit.input).await;
        let elapsed_ms = started.elapsed().as_millis();
        let (status, file_index) = match inspected {
            Ok(release) => {
                let context = if case.mentions_artist(&normalize(&release.name)) {
                    release.name.clone()
                } else {
                    format!("{} {}", hit.title, release.name)
                };
                let (judgment, index) = case.judge_files(
                    &context,
                    matches!(release.layout, inspect::ReleaseLayout::CueImageAlbum),
                    release
                        .tracks
                        .iter()
                        .map(|track| (track.file_index, track.title.as_str())),
                );
                let status = match judgment {
                    Judgment::Correct => InspectionStatus::Verified,
                    Judgment::Wrong => InspectionStatus::Wrong,
                    Judgment::Ambiguous => InspectionStatus::Ambiguous,
                };
                (status, index)
            }
            Err(error) if error.to_ascii_lowercase().contains("timed out") => {
                (InspectionStatus::Timeout, None)
            }
            Err(_) => (InspectionStatus::Failed, None),
        };
        result.inspections.push(InspectionOutcome {
            rank: offset + 1,
            provider: Provider::parse(&hit.provider),
            input: if hit.input.starts_with("magnet:") {
                InputKind::Magnet
            } else {
                InputKind::TorrentFile
            },
            status,
            elapsed_ms,
            file_index,
        });
    }
    result
}

fn percentile(values: impl IntoIterator<Item = u128>, percentile: usize) -> Option<u128> {
    let mut sorted = values.into_iter().collect::<Vec<_>>();
    sorted.sort_unstable();
    let rank = (sorted.len() * percentile).div_ceil(100);
    rank.checked_sub(1)
        .and_then(|index| sorted.get(index))
        .copied()
}

fn latency(values: &[u128]) -> String {
    match (
        percentile(values.iter().copied(), 50),
        percentile(values.iter().copied(), 95),
    ) {
        (Some(median), Some(p95)) => format!("{median} / {p95} ms"),
        _ => "—".to_owned(),
    }
}

fn counts(cases: &[&CaseOutcome]) -> String {
    let measure = |f: fn(&CaseOutcome) -> bool| cases.iter().filter(|case| f(case)).count();
    let successful = measure(|case| {
        case.providers
            .iter()
            .any(|p| matches!(p.status, ProviderStatus::Hits | ProviderStatus::Empty))
    });
    let empty = measure(|case| {
        case.hits == 0
            && case
                .providers
                .iter()
                .any(|p| p.status == ProviderStatus::Empty)
    });
    let first = measure(|case| case.top_titles.first() == Some(&Judgment::Correct));
    let top_three = measure(|case| case.top_titles.contains(&Judgment::Correct));
    let search_ms = cases.iter().map(|case| case.search_ms).collect::<Vec<_>>();
    let inspection_ms = cases
        .iter()
        .flat_map(|case| case.inspections.iter().map(|attempt| attempt.elapsed_ms))
        .collect::<Vec<_>>();
    format!(
        "{} | {successful} | {empty} | {first} | {top_three} | {} | {} | {} | {} | {}",
        cases.len(),
        measure(CaseOutcome::verified),
        measure(CaseOutcome::wrong_match),
        measure(CaseOutcome::ambiguous),
        latency(&search_ms),
        latency(&inspection_ms),
    )
}

fn report_markdown(date: &str, cases: &[Case], results: &BTreeMap<String, CaseOutcome>) -> String {
    use std::fmt::Write;

    let mut report = format!("# Blacktape torrent metadata evaluation · {date}\n\n");
    report.push_str(concat!(
        "The run uses up to three distinct ranked releases per query. The title score uses the corpus’s catalog identity and version rules. ",
        "Album verification uses named audio files from the reference edition. Track verification requires one unambiguous original file index. ",
        "Both scores use metainfo evidence. Entries marked pending had no completed query checkpoint. Median and p95 use nearest rank.\n\n",
        "## Results\n\n",
        "| Scope | Run | At least one provider success | Empty with provider success | Correct top one | Correct top three | Verified | With a wrong match | Ambiguous and unverified | Search median / p95 | Inspect median / p95 |\n",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |\n",
    ));
    writeln!(
        report,
        "| All (80) | {} |",
        counts(&results.values().collect::<Vec<_>>())
    )
    .unwrap();
    for kind in [Kind::Album, Kind::Track] {
        let by_kind = cases
            .iter()
            .filter(|case| case.kind == kind)
            .filter_map(|case| results.get(case.id))
            .collect::<Vec<_>>();
        writeln!(report, "| {} (40) | {} |", kind.as_str(), counts(&by_kind)).unwrap();
    }

    report.push_str("\n## Provider outcomes\n\n| Provider | Scope | Responses | Hits | Empty | Rate limited | Timeout | Malformed | Unavailable | Correct top one | Correct top three |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for provider in Provider::INDEXERS {
        for kind in [None, Some(Kind::Album), Some(Kind::Track)] {
            let outcomes = cases
                .iter()
                .filter(|case| kind.is_none_or(|kind| case.kind == kind))
                .filter_map(|case| results.get(case.id))
                .flat_map(|result| result.providers.iter())
                .filter(|outcome| provider == outcome.provider)
                .collect::<Vec<_>>();
            let status = |status| {
                outcomes
                    .iter()
                    .filter(|outcome| outcome.status == status)
                    .count()
            };
            let top_one = outcomes
                .iter()
                .filter(|outcome| outcome.top_titles.first() == Some(&Judgment::Correct))
                .count();
            let top_three = outcomes
                .iter()
                .filter(|outcome| outcome.top_titles.contains(&Judgment::Correct))
                .count();
            writeln!(
                report,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {top_one} | {top_three} |",
                provider.label(),
                kind.map_or("all", Kind::as_str),
                outcomes.len(),
                status(ProviderStatus::Hits),
                status(ProviderStatus::Empty),
                status(ProviderStatus::RateLimited),
                status(ProviderStatus::Timeout),
                status(ProviderStatus::Malformed),
                status(ProviderStatus::Unavailable),
            )
            .unwrap();
        }
    }

    report.push_str("\n## Inspections\n\n| Input | Scope | Attempts | Verified | Wrong | Ambiguous | Timed out | Failed | Median / p95 |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for input in [InputKind::Magnet, InputKind::TorrentFile] {
        for kind in [None, Some(Kind::Album), Some(Kind::Track)] {
            let attempts = cases
                .iter()
                .filter(|case| kind.is_none_or(|kind| kind == case.kind))
                .filter_map(|case| results.get(case.id))
                .flat_map(|result| result.inspections.iter())
                .filter(|attempt| attempt.input == input)
                .collect::<Vec<_>>();
            let status = |status| {
                attempts
                    .iter()
                    .filter(|attempt| attempt.status == status)
                    .count()
            };
            let times = attempts
                .iter()
                .map(|attempt| attempt.elapsed_ms)
                .collect::<Vec<_>>();
            writeln!(
                report,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                input.label(),
                kind.map_or("all", Kind::as_str),
                attempts.len(),
                status(InspectionStatus::Verified),
                status(InspectionStatus::Wrong),
                status(InspectionStatus::Ambiguous),
                status(InspectionStatus::Timeout),
                status(InspectionStatus::Failed),
                latency(&times),
            )
            .unwrap();
        }
    }

    report.push_str("\n## Per-query results\n\n| ID | Group | Artist | Query | Providers | Search ms | Hits | Top one | Top three | Inspections in rank order |\n| --- | --- | --- | --- | --- | ---: | ---: | --- | --- | --- |\n");
    for case in cases {
        let row = results.get(case.id).map_or_else(
            || "Pending | — | — | — | — | —".to_owned(),
            |outcome| {
                let providers = outcome
                    .providers
                    .iter()
                    .map(|provider| {
                        format!(
                            "{} {} ({})",
                            provider.provider.label(),
                            provider.status.label(),
                            provider.hit_count
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let first = match outcome.top_titles.first() {
                    Some(Judgment::Correct) => "correct",
                    Some(Judgment::Ambiguous) => "ambiguous",
                    Some(Judgment::Wrong) => "wrong",
                    None => "—",
                };
                let best = if outcome.top_titles.contains(&Judgment::Correct) {
                    "correct"
                } else if outcome.top_titles.contains(&Judgment::Ambiguous) {
                    "ambiguous"
                } else if outcome.top_titles.is_empty() {
                    "—"
                } else {
                    "wrong"
                };
                let inspections = outcome
                    .inspections
                    .iter()
                    .map(|attempt| {
                        let file = attempt
                            .file_index
                            .map_or_else(String::new, |index| format!(", file {index}"));
                        format!(
                            "#{} {} {} {} ({} ms{file})",
                            attempt.rank,
                            attempt.provider.label(),
                            attempt.input.label(),
                            attempt.status.label(),
                            attempt.elapsed_ms
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let status = if outcome.search_failed {
                    "Search operation failed"
                } else {
                    &providers
                };
                format!(
                    "{status} | {} | {} | {first} | {best} | {}",
                    outcome.search_ms,
                    outcome.hits,
                    if inspections.is_empty() {
                        "—"
                    } else {
                        &inspections
                    }
                )
            },
        );
        writeln!(
            report,
            "| {} | {} | {} | [{}]({}) | {row} |",
            case.id, case.group, case.artist, case.title, case.reference
        )
        .unwrap();
    }
    report
}

fn write_checkpoint(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[tokio::test]
#[ignore = "requires an explicit opt-in to live provider and metadata traffic"]
async fn live_torrent_evaluation() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        env::var_os("BLACKTAPE_TORRENT_EVAL_DIR")
            .ok_or("Set BLACKTAPE_TORRENT_EVAL_DIR to a run checkpoint directory")?,
    );
    let date = env::var("BLACKTAPE_TORRENT_EVAL_DATE")?;
    if date.len() != 10
        || !date.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return Err("BLACKTAPE_TORRENT_EVAL_DATE must use YYYY-MM-DD".into());
    }
    fs::create_dir_all(&directory)?;
    let cases = read_corpus();
    let mut results = BTreeMap::new();
    for case in &cases {
        let path = directory.join(format!("{}.json", case.id));
        match fs::read(path) {
            Ok(bytes) => {
                let outcome: CaseOutcome = serde_json::from_slice(&bytes)?;
                if outcome.id != case.id {
                    return Err("Checkpoint case ID does not match its filename".into());
                }
                results.insert(outcome.id.clone(), outcome);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let report = directory.join("results.md");
    write_checkpoint(&report, report_markdown(&date, &cases, &results).as_bytes())?;
    let mut pending = cases
        .iter()
        .filter(|case| !results.contains_key(case.id))
        .cloned()
        .collect::<Vec<_>>()
        .into_iter();
    let mut active = tokio::task::JoinSet::new();
    let mut next_search = tokio::time::Instant::now();
    loop {
        while active.len() < CONCURRENT_CASES {
            let Some(case) = pending.next() else {
                break;
            };
            tokio::time::sleep_until(next_search).await;
            active.spawn(async move { evaluate(&case).await });
            next_search = tokio::time::Instant::now() + CASE_INTERVAL;
        }
        let Some(outcome) = active.join_next().await else {
            break;
        };
        let outcome = outcome?;
        if outcome
            .providers
            .iter()
            .any(|provider| provider.status == ProviderStatus::RateLimited)
        {
            next_search = next_search.max(tokio::time::Instant::now() + RATE_LIMIT_COOLDOWN);
        }
        let id = outcome.id.clone();
        write_checkpoint(
            &directory.join(format!("{id}.json")),
            &serde_json::to_vec(&outcome)?,
        )?;
        results.insert(id.clone(), outcome);
        write_checkpoint(&report, report_markdown(&date, &cases, &results).as_bytes())?;
        eprintln!(
            "Torrent evaluation recorded {}/{} ({id})",
            results.len(),
            cases.len()
        );
    }
    Ok(())
}
