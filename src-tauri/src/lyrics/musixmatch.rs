//! Musixmatch provider — best coverage, supports synced (LRC) lyrics.

use super::common::{
    build_url, get_json, is_acceptable, is_trusted_https_url, track_matches, LyricsSource,
    BROWSER_USER_AGENT,
};
use serde_json::Value;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://apic.musixmatch.com/ws/1.1";
const APP_ID: &str = "android-player-v1.0";
const SOURCE: &str = "https://www.musixmatch.com";
const TOKEN_TTL: Duration = Duration::from_secs(10 * 60);

static TOKEN_CACHE: LazyLock<Mutex<Option<(String, Instant)>>> = LazyLock::new(|| Mutex::new(None));

fn cached_token() -> Option<String> {
    let guard = TOKEN_CACHE.lock().ok()?;
    guard
        .as_ref()
        .filter(|(_, at)| at.elapsed() < TOKEN_TTL)
        .map(|(t, _)| t.clone())
}

fn store_token(token: &str) {
    if let Ok(mut g) = TOKEN_CACHE.lock() {
        *g = Some((token.to_string(), Instant::now()));
    }
}

fn invalidate_token() {
    if let Ok(mut g) = TOKEN_CACHE.lock() {
        *g = None;
    }
}

async fn get_token(client: &reqwest::Client) -> Result<String, String> {
    if let Some(t) = cached_token() {
        return Ok(t);
    }
    let url = build_url(&format!("{API}/token.get"), &[("app_id", APP_ID)]);
    let val: Value = get_json(client, &url, BROWSER_USER_AGENT).await?;
    let token = val
        .pointer("/message/body/user_token")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Failed to parse Musixmatch user token".to_string())?;
    store_token(token);
    Ok(token.to_string())
}

/// Removes the "NOT for Commercial use" footer Musixmatch appends to plain lyrics.
pub fn clean_musixmatch_lyrics(text: &str) -> String {
    let cut = text
        .find("******* This Lyrics is NOT for Commercial use")
        .unwrap_or(text.len());
    text[..cut].trim().to_string()
}

async fn call(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    extra: &[(&str, &str)],
) -> Option<Value> {
    let mut params: Vec<(&str, &str)> = vec![("app_id", APP_ID), ("usertoken", token)];
    params.extend_from_slice(extra);
    let url = build_url(&format!("{API}/{endpoint}"), &params);
    get_json(client, &url, BROWSER_USER_AGENT).await.ok()
}

fn body_text<'a>(val: &'a Value, path: &str) -> Option<&'a str> {
    val.pointer(&format!("/message/body/{path}"))?.as_str()
}

fn accept(raw: Option<&str>, source: &str) -> Option<LyricsSource> {
    let cleaned = clean_musixmatch_lyrics(raw?);
    is_acceptable(&cleaned).then(|| LyricsSource {
        lyrics: cleaned,
        source: source.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Richsync (word-level timing) -> Enhanced LRC
// ---------------------------------------------------------------------------

fn lrc_stamp(secs: f64) -> String {
    let cs = (secs.max(0.0) * 100.0).round() as u64;
    format!("{:02}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

/// Converts Musixmatch `richsync_body` JSON into Enhanced LRC (A2 extension):
///
/// `[00:18.50]<00:18.50>Take <00:18.66>a <00:18.74>step <00:20.91>`
///
/// Each word is preceded by its start time and the line ends with an end
/// stamp. Every line still starts with a normal `[mm:ss.xx]`, so players that
/// only understand plain LRC (and `is_synced`) keep working.
pub fn richsync_to_enhanced_lrc(body: &str) -> Option<String> {
    let lines: Vec<Value> = serde_json::from_str(body).ok()?;
    let mut out: Vec<String> = Vec::new();
    let mut total_words = 0usize;

    for line in &lines {
        let ts = line.get("ts")?.as_f64()?;
        let te = line.get("te").and_then(|t| t.as_f64()).unwrap_or(ts);
        let tokens = line.get("l").and_then(|l| l.as_array());

        // Group tokens into words: whitespace separates words, tokens may be
        // whole words or single letters (letter_by_letter).
        let mut words: Vec<(f64, String)> = Vec::new();
        let mut cur: Option<(f64, String)> = None;
        for tok in tokens.into_iter().flatten() {
            let c = tok.get("c").and_then(|c| c.as_str()).unwrap_or("");
            let o = tok.get("o").and_then(|o| o.as_f64()).unwrap_or(0.0);
            for ch in c.chars() {
                if ch.is_whitespace() {
                    if let Some(w) = cur.take() {
                        words.push(w);
                    }
                } else {
                    cur.get_or_insert((ts + o, String::new())).1.push(ch);
                }
            }
        }
        if let Some(w) = cur.take() {
            words.push(w);
        }

        if words.is_empty() {
            out.push(format!("[{}]", lrc_stamp(ts)));
            continue;
        }

        let mut text = format!("[{}]", lrc_stamp(ts));
        let mut prev = ts;
        for (t, w) in &words {
            let t = t.max(prev); // keep word times monotonic
            prev = t;
            text.push_str(&format!("<{}>{} ", lrc_stamp(t), w));
        }
        text.push_str(&format!("<{}>", lrc_stamp(te.max(prev))));
        total_words += words.len();
        out.push(text);
    }

    (total_words > 0).then(|| out.join("
"))
}

/// A track returned by `track.search` that matches the requested artist/title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub share_url: String,
    pub has_subtitles: bool,
    pub has_richsync: bool,
}

/// Filters `track.search` results down to tracks that really are the requested
/// song (matcher.* endpoints are not used: they silently return a similar
/// song). Tracks with timed lyrics come first.
pub fn pick_candidates(search: &Value, artist: &str, title: &str) -> Vec<Candidate> {
    let Some(list) = search
        .pointer("/message/body/track_list")
        .and_then(|l| l.as_array())
    else {
        return Vec::new();
    };

    let mut out: Vec<Candidate> = list
        .iter()
        .filter_map(|item| {
            let tr = item.get("track")?;
            let name = tr.get("track_name").and_then(|n| n.as_str()).unwrap_or("");
            let art = tr.get("artist_name").and_then(|n| n.as_str()).unwrap_or("");
            if !track_matches(artist, title, art, name) {
                return None;
            }
            let id = tr.get("track_id").and_then(|i| i.as_i64())?.to_string();
            let share_url = tr
                .get("track_share_url")
                .and_then(|u| u.as_str())
                // Shown/opened in the UI: only accept real musixmatch https links.
                .filter(|u| is_trusted_https_url(u, &["musixmatch.com"]))
                .unwrap_or(SOURCE)
                .to_string();
            let has_subtitles = tr.get("has_subtitles").and_then(|h| h.as_i64()) == Some(1);
            let has_richsync = tr.get("has_richsync").and_then(|h| h.as_i64()) == Some(1);
            Some(Candidate {
                id,
                share_url,
                has_subtitles,
                has_richsync,
            })
        })
        .collect();
    out.sort_by_key(|c| !c.has_subtitles); // stable: keeps relevance order otherwise
    out
}

const MAX_CANDIDATES: usize = 3;

pub async fn fetch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<LyricsSource, String> {
    let token = get_token(client).await?;

    let search = call(
        client,
        "track.search",
        &token,
        &[
            ("q_artist", artist),
            ("q_track", title),
            ("f_has_lyrics", "1"),
            ("page_size", "5"),
        ],
    )
    .await
    .ok_or_else(|| "Musixmatch: search failed".to_string())?;

    let mut candidates = pick_candidates(&search, artist, title);
    candidates.truncate(MAX_CANDIDATES);

    // 0. Word-level timing (richsync), converted to Enhanced LRC — best quality.
    //    Any failure just falls through to the regular line-level lyrics below.
    for c in candidates.iter().filter(|c| c.has_richsync) {
        let p = [("track_id", c.id.as_str())];
        if let Some(v) = call(client, "track.richsync.get", &token, &p).await {
            let lrc = body_text(&v, "richsync/richsync_body").and_then(richsync_to_enhanced_lrc);
            if let Some(lrc) = lrc.filter(|l| is_acceptable(l)) {
                return Ok(LyricsSource {
                    lyrics: lrc,
                    source: c.share_url.clone(),
                });
            }
        }
    }

    // 1. Timed lyrics (LRC)
    for c in candidates.iter().filter(|c| c.has_subtitles) {
        let p = [("track_id", c.id.as_str()), ("subtitle_format", "lrc")];
        if let Some(v) = call(client, "track.subtitle.get", &token, &p).await {
            if let Some(l) = accept(body_text(&v, "subtitle/subtitle_body"), &c.share_url) {
                return Ok(l);
            }
        }
    }

    // 2. Plain lyrics
    for c in &candidates {
        let p = [("track_id", c.id.as_str())];
        if let Some(v) = call(client, "track.lyrics.get", &token, &p).await {
            if let Some(l) = accept(body_text(&v, "lyrics/lyrics_body"), &c.share_url) {
                return Ok(l);
            }
        }
    }

    if candidates.is_empty() {
        // Search worked but nothing matched. Don't blame the token.
        return Err("Musixmatch: no matching track found".to_string());
    }
    // Possibly an expired/blocked token — don't keep reusing it.
    invalidate_token();
    Err("Musixmatch: no lyrics found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_removes_commercial_use_footer() {
        let input = "Hello\nWorld\n\n******* This Lyrics is NOT for Commercial use *******\n(140961)";
        assert_eq!(clean_musixmatch_lyrics(input), "Hello\nWorld");
    }

    #[test]
    fn clean_only_trims_text_without_footer() {
        assert_eq!(clean_musixmatch_lyrics("  hello\nworld  \n"), "hello\nworld");
    }

    #[test]
    fn clean_keeps_genuine_asterisks_in_lyrics() {
        assert_eq!(
            clean_musixmatch_lyrics("Some lyrics\n*******\nmore lyrics"),
            "Some lyrics\n*******\nmore lyrics"
        );
    }

    #[test]
    fn clean_empty_input_returns_empty() {
        assert_eq!(clean_musixmatch_lyrics(""), "");
    }
}

#[cfg(test)]
mod candidate_tests {
    use super::*;
    use serde_json::json;

    fn track(id: i64, name: &str, artist: &str, subs: i64, url: &str) -> Value {
        json!({"track": {"track_id": id, "track_name": name, "artist_name": artist,
                         "has_subtitles": subs, "track_share_url": url}})
    }

    fn search(items: Vec<Value>) -> Value {
        json!({"message": {"body": {"track_list": items}}})
    }

    #[test]
    fn wrong_song_from_search_is_rejected() {
        let v = search(vec![track(
            1,
            "Widow's Walk",
            "Larkin Poe",
            1,
            "https://www.musixmatch.com/lyrics/Larkin-Poe/Widow-s-Walk",
        )]);
        assert!(pick_candidates(&v, "Christopher Larkin", "Widow").is_empty());
    }

    #[test]
    fn matching_tracks_are_kept_and_timed_ones_come_first() {
        let v = search(vec![
            track(1, "Widow", "Christopher Larkin", 0, "https://www.musixmatch.com/a"),
            track(2, "Widow", "Christopher Larkin", 1, "https://www.musixmatch.com/b"),
            track(3, "Widow's Walk", "Larkin Poe", 1, "https://www.musixmatch.com/c"),
        ]);
        let c = pick_candidates(&v, "Christopher Larkin", "Widow");
        assert_eq!(c.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
        assert!(c[0].has_subtitles && !c[1].has_subtitles);
    }

    #[test]
    fn untrusted_share_url_falls_back_to_musixmatch_home() {
        let v = search(vec![track(1, "Widow", "Christopher Larkin", 1, "http://evil.com/x")]);
        assert_eq!(pick_candidates(&v, "Christopher Larkin", "Widow")[0].share_url, SOURCE);
    }

    #[test]
    fn malformed_responses_give_no_candidates() {
        assert!(pick_candidates(&json!({}), "a", "b").is_empty());
        assert!(pick_candidates(&search(vec![json!({"track": {}})]), "a", "b").is_empty());
    }
}

#[cfg(test)]
mod richsync_tests {
    use super::*;
    use crate::lyrics::common::is_synced;

    const BODY: &str = r#"[
      {"ts": 18.5, "te": 20.91, "l": [{"c":"Take","o":0},{"c":" ","o":0.1},{"c":"a","o":0.16},{"c":" ","o":0.2},{"c":"step","o":0.24}], "x": "Take a step"},
      {"ts": 21.0, "te": 22.0, "l": [], "x": ""},
      {"ts": 70.25, "te": 72.0, "l": [{"c":"Hi","o":0},{"c":" ","o":0.3},{"c":"there","o":0.5}], "x": "Hi there"}
    ]"#;

    #[test]
    fn converts_words_to_enhanced_lrc() {
        let lrc = richsync_to_enhanced_lrc(BODY).unwrap();
        let lines: Vec<&str> = lrc.lines().collect();
        assert_eq!(
            lines[0],
            "[00:18.50]<00:18.50>Take <00:18.66>a <00:18.74>step <00:20.91>"
        );
        assert_eq!(lines[1], "[00:21.00]"); // empty line kept as a gap
        assert_eq!(lines[2], "[01:10.25]<01:10.25>Hi <01:10.75>there <01:12.00>");
        assert!(is_synced(&lrc));
    }

    #[test]
    fn letter_by_letter_tokens_are_grouped_into_words() {
        let body = r#"[{"ts":1.0,"te":2.0,"l":[{"c":"H","o":0},{"c":"i","o":0.1},{"c":" ","o":0.2},{"c":"y","o":0.3},{"c":"o","o":0.4}],"x":"Hi yo"}]"#;
        assert_eq!(
            richsync_to_enhanced_lrc(body).unwrap(),
            "[00:01.00]<00:01.00>Hi <00:01.30>yo <00:02.00>"
        );
    }

    #[test]
    fn tokens_with_embedded_spaces_are_split_into_words() {
        let body = r#"[{"ts":0.0,"te":1.0,"l":[{"c":"one two","o":0.0}],"x":"one two"}]"#;
        assert_eq!(
            richsync_to_enhanced_lrc(body).unwrap(),
            "[00:00.00]<00:00.00>one <00:00.00>two <00:01.00>"
        );
    }

    #[test]
    fn word_times_never_go_backwards_and_end_is_not_before_last_word() {
        let body = r#"[{"ts":5.0,"te":5.1,"l":[{"c":"a","o":1.0},{"c":" ","o":1.1},{"c":"b","o":0.5}],"x":"a b"}]"#;
        let lrc = richsync_to_enhanced_lrc(body).unwrap();
        assert_eq!(lrc, "[00:05.00]<00:06.00>a <00:06.00>b <00:06.00>");
    }

    #[test]
    fn invalid_or_wordless_bodies_give_none() {
        assert_eq!(richsync_to_enhanced_lrc("not json"), None);
        assert_eq!(richsync_to_enhanced_lrc("{}"), None);
        assert_eq!(richsync_to_enhanced_lrc("[]"), None);
        assert_eq!(richsync_to_enhanced_lrc(r#"[{"ts":1.0,"te":2.0,"l":[],"x":""}]"#), None);
        // Missing ts makes the body unusable instead of producing garbage.
        assert_eq!(richsync_to_enhanced_lrc(r#"[{"te":2.0,"l":[{"c":"a","o":0}]}]"#), None);
    }

    #[test]
    fn stamp_formatting_rounds_to_centiseconds() {
        assert_eq!(lrc_stamp(0.0), "00:00.00");
        assert_eq!(lrc_stamp(59.999), "01:00.00");
        assert_eq!(lrc_stamp(125.456), "02:05.46");
        assert_eq!(lrc_stamp(-3.0), "00:00.00");
    }
}
