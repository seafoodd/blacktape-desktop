//! Lyrics fetching. One file per provider; this module orchestrates them.
//!
//! Priority: lyrics WITH timestamps (LRC) always win over plain lyrics.
//!   1. Musixmatch   (synced-capable, best coverage)
//!   2. LRCLIB       (synced-capable)
//!   3. lyrics.ovh   (plain only)
//!   4. Genius       (plain only)
//! If neither synced-capable provider returns timestamps, the first plain
//! result in that order is used, then the plain-only providers are tried.

mod common;
mod genius;
mod lrclib;
mod lyrics_ovh;
mod musixmatch;

use std::future::Future;

pub use common::LyricsSource;
use common::{build_client, clean_track_title, is_synced, INSTRUMENTAL_LYRICS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Musixmatch,
    Lrclib,
    LyricsOvh,
    Genius,
}

impl Provider {
    /// Providers in priority order.
    pub const PRIORITY: [Provider; 4] = [
        Provider::Musixmatch,
        Provider::Lrclib,
        Provider::LyricsOvh,
        Provider::Genius,
    ];

    pub fn supports_synced(self) -> bool {
        matches!(self, Provider::Musixmatch | Provider::Lrclib)
    }

    pub fn parse(name: &str) -> Option<Provider> {
        match name.trim().to_lowercase().as_str() {
            "musixmatch" => Some(Provider::Musixmatch),
            "lrclib" => Some(Provider::Lrclib),
            "lyrics.ovh" | "lyrics_ovh" | "lyricsovh" => Some(Provider::LyricsOvh),
            "genius" => Some(Provider::Genius),
            _ => None,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Provider::Musixmatch => "Musixmatch",
            Provider::Lrclib => "LRCLIB",
            Provider::LyricsOvh => "lyrics.ovh",
            Provider::Genius => "Genius",
        }
    }
}

/// Original title first, then the cleaned one (no "(Official Video)", "(feat. X)").
fn title_variants(title: &str) -> Vec<String> {
    let cleaned = clean_track_title(title);
    let mut v = vec![title.to_string()];
    if !cleaned.is_empty() && cleaned != title {
        v.push(cleaned);
    }
    v
}

/// Queries one provider over all title variants. Returns the first synced
/// result, else the first plain one.
async fn fetch_provider(
    client: &reqwest::Client,
    provider: Provider,
    artist: &str,
    titles: &[String],
    album: Option<&str>,
    duration_secs: Option<u64>,
) -> Result<LyricsSource, String> {
    let mut fallback: Option<LyricsSource> = None;
    for title in titles {
        let res = match provider {
            Provider::Musixmatch => musixmatch::fetch(client, artist, title).await,
            Provider::Lrclib => lrclib::fetch(client, artist, title, album, duration_secs).await,
            Provider::LyricsOvh => lyrics_ovh::fetch(client, artist, title).await,
            Provider::Genius => genius::fetch(client, artist, title).await,
        };
        if let Ok(r) = res {
            if is_synced(&r.lyrics) && r.lyrics != INSTRUMENTAL_LYRICS {
                return Ok(r);
            }
            fallback.get_or_insert(r);
        }
    }
    fallback.ok_or_else(|| format!("No lyrics found on {}", provider.display_name()))
}

/// Core priority logic, independent of the network (see tests).
async fn run_priority<F, Fut>(fetch: F) -> Result<LyricsSource, String>
where
    F: Fn(Provider) -> Fut,
    Fut: Future<Output = Result<LyricsSource, String>>,
{
    let mut plain: Option<LyricsSource> = None;
    let mut instrumental: Option<LyricsSource> = None;

    // Phase 1: providers that can return timestamps. Synced wins immediately.
    for p in Provider::PRIORITY.iter().copied().filter(|p| p.supports_synced()) {
        if let Ok(res) = fetch(p).await {
            if res.lyrics == INSTRUMENTAL_LYRICS {
                instrumental.get_or_insert(res);
            } else if is_synced(&res.lyrics) {
                return Ok(res);
            } else {
                plain.get_or_insert(res);
            }
        }
    }
    if let Some(r) = plain.or(instrumental) {
        return Ok(r);
    }

    // Phase 2: plain-only providers.
    for p in Provider::PRIORITY.iter().copied().filter(|p| !p.supports_synced()) {
        if let Ok(res) = fetch(p).await {
            return Ok(res);
        }
    }

    Err("Lyrics not found across any provider".to_string())
}

/// Fetches lyrics trying providers in priority order (timestamps first).
pub async fn fetch_lyrics(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<LyricsSource, String> {
    let client = build_client();
    let duration_secs = duration_ms.map(|d| d / 1000);
    let titles = title_variants(title);

    run_priority(|p| fetch_provider(&client, p, artist, &titles, album, duration_secs)).await
}

/// Fetches lyrics from a specific provider ("lrclib", "musixmatch",
/// "lyrics.ovh", "genius"); anything else (e.g. "auto") uses the priority order.
pub async fn fetch_lyrics_from_provider(
    provider: &str,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<LyricsSource, String> {
    let Some(p) = Provider::parse(provider) else {
        return fetch_lyrics(artist, title, album, duration_ms).await;
    };
    let client = build_client();
    let duration_secs = duration_ms.map(|d| d / 1000);
    let titles = title_variants(title);
    fetch_provider(&client, p, artist, &titles, album, duration_secs).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn src(lyrics: &str, source: &str) -> LyricsSource {
        LyricsSource {
            lyrics: lyrics.to_string(),
            source: source.to_string(),
        }
    }

    const SYNCED: &str = "[00:01.00] hello\n[00:02.00] world";
    const PLAIN: &str = "hello\nworld";

    /// Runs the priority logic against canned provider answers, returning the
    /// result and the order in which providers were queried.
    async fn run(
        answers: Vec<(Provider, Result<LyricsSource, String>)>,
    ) -> (Result<LyricsSource, String>, Vec<Provider>) {
        let calls = Mutex::new(Vec::new());
        let res = run_priority(|p| {
            calls.lock().unwrap().push(p);
            let ans = answers
                .iter()
                .find(|(ap, _)| *ap == p)
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| Err("none".into()));
            async move { ans }
        })
        .await;
        (res, calls.into_inner().unwrap())
    }

    #[test]
    fn priority_order_is_musixmatch_first() {
        assert_eq!(Provider::PRIORITY[0], Provider::Musixmatch);
        assert_eq!(Provider::PRIORITY[1], Provider::Lrclib);
    }

    #[tokio::test]
    async fn musixmatch_synced_wins_and_stops() {
        let (res, calls) = run(vec![
            (Provider::Musixmatch, Ok(src(SYNCED, "mm"))),
            (Provider::Lrclib, Ok(src(SYNCED, "lrclib"))),
        ])
        .await;
        assert_eq!(res.unwrap().source, "mm");
        assert_eq!(calls, vec![Provider::Musixmatch]);
    }

    #[tokio::test]
    async fn synced_from_lrclib_beats_plain_from_musixmatch() {
        let (res, calls) = run(vec![
            (Provider::Musixmatch, Ok(src(PLAIN, "mm"))),
            (Provider::Lrclib, Ok(src(SYNCED, "lrclib"))),
            (Provider::Genius, Ok(src(PLAIN, "genius"))),
        ])
        .await;
        assert_eq!(res.unwrap().source, "lrclib");
        assert_eq!(calls, vec![Provider::Musixmatch, Provider::Lrclib]);
    }

    #[tokio::test]
    async fn plain_fallback_follows_priority_without_trying_plain_only_providers() {
        let (res, calls) = run(vec![
            (Provider::Musixmatch, Ok(src(PLAIN, "mm"))),
            (Provider::Lrclib, Ok(src(PLAIN, "lrclib"))),
            (Provider::LyricsOvh, Ok(src(PLAIN, "ovh"))),
        ])
        .await;
        assert_eq!(res.unwrap().source, "mm");
        assert_eq!(calls, vec![Provider::Musixmatch, Provider::Lrclib]);
    }

    #[tokio::test]
    async fn plain_only_providers_are_used_last_in_order() {
        let (res, calls) = run(vec![
            (Provider::LyricsOvh, Err("no".into())),
            (Provider::Genius, Ok(src(PLAIN, "genius"))),
        ])
        .await;
        assert_eq!(res.unwrap().source, "genius");
        assert_eq!(
            calls,
            vec![
                Provider::Musixmatch,
                Provider::Lrclib,
                Provider::LyricsOvh,
                Provider::Genius
            ]
        );
    }

    #[tokio::test]
    async fn instrumental_marker_is_lowest_priority_of_phase_one() {
        let (res, _) = run(vec![
            (Provider::Musixmatch, Ok(src(INSTRUMENTAL_LYRICS, "mm"))),
            (Provider::Lrclib, Ok(src(PLAIN, "lrclib"))),
        ])
        .await;
        assert_eq!(res.unwrap().source, "lrclib");

        let (res, calls) = run(vec![(
            Provider::Lrclib,
            Ok(src(INSTRUMENTAL_LYRICS, "lrclib")),
        )])
        .await;
        assert_eq!(res.unwrap().lyrics, INSTRUMENTAL_LYRICS);
        assert_eq!(calls.len(), 2); // no network calls to plain-only providers
    }

    #[tokio::test]
    async fn errors_when_every_provider_fails() {
        let (res, calls) = run(vec![]).await;
        assert!(res.is_err());
        assert_eq!(calls.len(), 4);
    }

    #[test]
    fn provider_parse_accepts_aliases_and_rejects_auto() {
        assert_eq!(Provider::parse(" LRCLIB "), Some(Provider::Lrclib));
        assert_eq!(Provider::parse("lyrics_ovh"), Some(Provider::LyricsOvh));
        assert_eq!(Provider::parse("lyricsovh"), Some(Provider::LyricsOvh));
        assert_eq!(Provider::parse("auto"), None);
    }

    #[test]
    fn title_variants_adds_cleaned_title_only_when_different() {
        assert_eq!(title_variants("Song"), vec!["Song"]);
        assert_eq!(
            title_variants("Song (Official Audio)"),
            vec!["Song (Official Audio)", "Song"]
        );
    }
}
