//! Shared types and helpers for all lyrics providers.

use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;

pub const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// Identifies the app to open APIs (LRCLIB, later MusicBrainz), which ask for name, version and a contact url.
/// The version follows Cargo.toml, so it never goes stale.
pub const APP_USER_AGENT: &str = concat!(
    "Blacktape/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/seafoodd/blacktape-desktop)"
);

/// Hard cap on any single provider response body.
pub const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
/// Hard cap on the lyrics text we are willing to accept/store.
pub const MAX_LYRICS_CHARS: usize = 200_000;
/// Marker lyrics for instrumental tracks.
pub const INSTRUMENTAL_LYRICS: &str = "[00:00.00] Instrumental";

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct LyricsSource {
    pub lyrics: String,
    pub source: String,
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        // Providers are all HTTPS; this also rejects redirects to plain http.
        .https_only(true)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn build_url(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = base.to_string();
    if !params.is_empty() {
        url.push('?');
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(&urlencoding::encode(k));
            url.push('=');
            url.push_str(&urlencoding::encode(v));
        }
    }
    url
}

/// Strips the request URL from a reqwest error. Request URLs can contain
/// tokens (e.g. Musixmatch `usertoken`) that must never reach logs or the UI.
pub fn err_string(e: reqwest::Error) -> String {
    e.without_url().to_string()
}

/// Reads a response body, refusing anything larger than `max` bytes.
pub async fn read_body_limited(mut res: reqwest::Response, max: usize) -> Result<String, String> {
    if let Some(len) = res.content_length() {
        if len as usize > max {
            return Err("response too large".to_string());
        }
    }
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(err_string)? {
        if buf.len() + chunk.len() > max {
            return Err("response too large".to_string());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// GET `url` and return the (size-limited) body as text. Fails on non-2xx.
pub async fn get_text(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
) -> Result<String, String> {
    let res = client
        .get(url)
        .header("User-Agent", user_agent)
        .send()
        .await
        .map_err(err_string)?;
    if !res.status().is_success() {
        return Err(format!("HTTP {}", res.status()));
    }
    read_body_limited(res, MAX_BODY_BYTES).await
}

/// GET `url` and deserialize the (size-limited) body as JSON.
pub async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
) -> Result<T, String> {
    let body = get_text(client, url, user_agent).await?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// True only for `https` URLs without credentials/custom port whose host is
/// one of `allowed_hosts` or a subdomain of one.
pub fn is_trusted_https_url(raw: &str, allowed_hosts: &[&str]) -> bool {
    let Ok(u) = url::Url::parse(raw) else {
        return false;
    };
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    allowed_hosts
        .iter()
        .any(|a| host == *a || host.ends_with(&format!(".{a}")))
}

static SYNCED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*\[\d{1,3}:\d{2}(?:[.:]\d{1,3})?\]").unwrap());

/// True when the lyrics contain LRC timestamps (`[mm:ss.xx]`).
pub fn is_synced(lyrics: &str) -> bool {
    SYNCED_RE.is_match(lyrics)
}

/// Basic sanity check applied to every provider result.
pub fn is_acceptable(lyrics: &str) -> bool {
    !lyrics.trim().is_empty() && lyrics.chars().count() <= MAX_LYRICS_CHARS
}

static TITLE_NOISE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\s*[\(\[](official\s*(music\s*)?video|official\s*audio|audio|visualizer|lyric\s*video|lyrics|hd|4k|remastered|remaster)[\)\]]",
    )
    .unwrap()
});
static TITLE_FEAT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*[\(\[](feat|ft)\.?\s+[^\)\]]+[\)\]]").unwrap());

pub fn clean_track_title(title: &str) -> String {
    let cleaned = TITLE_NOISE_RE.replace_all(title, "");
    let cleaned = TITLE_FEAT_RE.replace_all(&cleaned, "");
    cleaned.trim().to_string()
}

// ---------------------------------------------------------------------------
// Match validation: fuzzy provider searches may return a different song
// ("Widow" by Christopher Larkin -> "Widow's Walk" by Larkin Poe).
// ---------------------------------------------------------------------------

/// Lowercased alphanumeric characters only.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Artist plausibly the same: equal, one contains the other (e.g. "A & B"
/// vs "A", min 3 chars) or very similar spelling. Unknown (empty) matches.
pub fn artist_matches(wanted: &str, found: &str) -> bool {
    let (w, f) = (normalize(wanted), normalize(found));
    if w.is_empty() || f.is_empty() {
        return true;
    }
    if w == f {
        return true;
    }
    let (short, long) = if w.len() <= f.len() { (&w, &f) } else { (&f, &w) };
    if short.chars().count() >= 3 && long.contains(short.as_str()) {
        return true;
    }
    strsim::normalized_levenshtein(&w, &f) >= 0.8
}

/// Title is the same song: equal after removing "(feat. X)", "(Official Audio)"
/// etc., or near-identical spelling. Deliberately NOT "contains":
/// "Widow" must not match "Widow's Walk".
pub fn title_matches(wanted: &str, found: &str) -> bool {
    let w = normalize(&clean_track_title(wanted));
    let f = normalize(&clean_track_title(found));
    if w.is_empty() || f.is_empty() {
        return true;
    }
    w == f || strsim::normalized_levenshtein(&w, &f) >= 0.85
}

/// Both artist and title must match.
pub fn track_matches(
    wanted_artist: &str,
    wanted_title: &str,
    found_artist: &str,
    found_title: &str,
) -> bool {
    artist_matches(wanted_artist, found_artist) && title_matches(wanted_title, found_title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_track_title_strips_official_video_tag() {
        assert_eq!(
            clean_track_title("Never Gonna Give You Up (Official Music Video)"),
            "Never Gonna Give You Up"
        );
    }

    #[test]
    fn clean_track_title_strips_bracketed_official_audio() {
        assert_eq!(clean_track_title("Song Name [Official Audio]"), "Song Name");
    }

    #[test]
    fn clean_track_title_strips_feat_in_parens() {
        assert_eq!(clean_track_title("Track (feat. Another Artist)"), "Track");
    }

    #[test]
    fn clean_track_title_keeps_bare_ft_suffix() {
        assert_eq!(
            clean_track_title("Track ft. Another Artist"),
            "Track ft. Another Artist"
        );
    }

    #[test]
    fn clean_track_title_keeps_plain_title() {
        assert_eq!(clean_track_title("Simple Song"), "Simple Song");
    }

    #[test]
    fn is_synced_detects_lrc_timestamp() {
        assert!(is_synced("[01:23.45] hi"));
        assert!(is_synced("[1:05] x"));
    }

    #[test]
    fn is_synced_detects_timestamp_on_later_line() {
        assert!(is_synced("Intro text\n[00:12.00] first line"));
    }

    #[test]
    fn is_synced_rejects_plain_text_and_section_markers() {
        assert!(!is_synced("plain text"));
        assert!(!is_synced("[Chorus]"));
        assert!(!is_synced("[Intro]"));
    }

    #[test]
    fn is_acceptable_rejects_empty_and_whitespace() {
        assert!(!is_acceptable(""));
        assert!(!is_acceptable("   \n\t  "));
    }

    #[test]
    fn is_acceptable_accepts_normal_lyrics() {
        assert!(is_acceptable("normal lyrics"));
    }

    #[test]
    fn is_acceptable_rejects_over_max_chars() {
        let too_long = "a".repeat(MAX_LYRICS_CHARS + 1);
        assert!(!is_acceptable(&too_long));
    }

    #[test]
    fn trusted_url_accepts_host_and_subdomain() {
        assert!(is_trusted_https_url("https://genius.com/x-lyrics", &["genius.com"]));
        assert!(is_trusted_https_url("https://www.genius.com/a", &["genius.com"]));
    }

    #[test]
    fn trusted_url_rejects_unsafe_or_foreign_urls() {
        let allowed = &["genius.com"];
        assert!(!is_trusted_https_url("http://genius.com/x", allowed));
        assert!(!is_trusted_https_url("https://evilgenius.com/x", allowed));
        assert!(!is_trusted_https_url("https://genius.com.evil.io/x", allowed));
        assert!(!is_trusted_https_url("https://user:pw@genius.com/x", allowed));
        assert!(!is_trusted_https_url("https://genius.com:8443/x", allowed));
        assert!(!is_trusted_https_url("not a url", allowed));
        assert!(!is_trusted_https_url("https://evil.com/?u=genius.com", allowed));
    }

    #[test]
    fn build_url_encodes_values_and_joins_params() {
        let url = build_url(
            "https://example.com/api",
            &[("q", "a b"), ("t", "a&b")],
        );
        assert_eq!(url, "https://example.com/api?q=a%20b&t=a%26b");
    }

    #[test]
    fn build_url_without_params_has_no_query() {
        assert_eq!(build_url("https://example.com/api", &[]), "https://example.com/api");
    }
}

#[cfg(test)]
mod match_tests {
    use super::*;

    #[test]
    fn widow_does_not_match_widows_walk_by_larkin_poe() {
        // Regression: Musixmatch returned the wrong song.
        assert!(!title_matches("Widow", "Widow's Walk"));
        assert!(!artist_matches("Christopher Larkin", "Larkin Poe"));
        assert!(!track_matches(
            "Christopher Larkin",
            "Widow",
            "Larkin Poe",
            "Widow's Walk"
        ));
        assert!(track_matches("Christopher Larkin", "Widow", "Christopher Larkin", "Widow"));
    }

    #[test]
    fn title_matching_ignores_noise_case_and_punctuation() {
        assert!(title_matches("Song (Official Audio)", "song"));
        assert!(title_matches("Track (feat. X)", "Track"));
        assert!(title_matches("Don't Stop", "Dont Stop"));
        assert!(!title_matches("Love", "Love Story"));
    }

    #[test]
    fn artist_matching_tolerates_collaborations_and_accents_but_not_unrelated() {
        assert!(artist_matches("Daft Punk", "Daft Punk feat. Pharrell Williams"));
        assert!(artist_matches("Beyonce", "Beyoncé"));
        assert!(artist_matches("", "Anyone"));
        assert!(!artist_matches("AC/DC", "Abba"));
        // Too short to count as containment.
        assert!(!artist_matches("Al", "Metallica"));
    }
}
