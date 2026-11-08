//! Genius provider — scrapes the lyrics page (plain, unsynced lyrics only).
//!
//! Genius wraps lyrics in `<div data-lyrics-container="true">` blocks. The first
//! block also contains page chrome (contributors, translations, song
//! description with "Read More", ads, ...) which is flagged with
//! `data-exclude-from-selection="true"` or recognisable class names. We remove
//! those sub-trees before converting the HTML to text.

use super::common::{
    artist_matches, build_url, get_json, get_text, is_acceptable, is_trusted_https_url,
    title_matches, LyricsSource, BROWSER_USER_AGENT,
};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const CONTAINER_ATTR: &str = "data-lyrics-container=\"true\"";
const GENIUS_HOSTS: &[&str] = &["genius.com"];

/// If any of these appear inside a `<div ...>` opening tag, the whole div is dropped.
const EXCLUDED_TAG_MARKERS: &[&str] = &[
    "data-exclude-from-selection=\"true\"",
    "LyricsHeader",
    "LyricsFooter",
    "Lyrics__Footer",
    "InreadContainer",
    "PrimisPlayer",
];

static BR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<script\b[^>]*>.*?</script\s*>").unwrap());
static STYLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style\b[^>]*>.*?</style\s*>").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
/// "133 Contributors..." header glued to the start of the lyrics.
static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d[\d.,]*[KkMm]?\s*Contributors?").unwrap());

// ---------------------------------------------------------------------------
// Tiny tag-aware HTML helpers (byte offsets always sit on ASCII '<' / '>')
// ---------------------------------------------------------------------------

fn starts_with_ci(b: &[u8], i: usize, needle: &[u8]) -> bool {
    b.len() >= i + needle.len() && b[i..i + needle.len()].eq_ignore_ascii_case(needle)
}

fn is_div_open(b: &[u8], i: usize) -> bool {
    starts_with_ci(b, i, b"<div")
        && b.get(i + 4)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/')
}

fn is_div_close(b: &[u8], i: usize) -> bool {
    starts_with_ci(b, i, b"</div")
        && b.get(i + 5)
            .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>')
}

/// Index of the `>` closing the tag that starts at `start` (quote-aware).
fn find_tag_end(b: &[u8], start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, &c) in b.iter().enumerate().skip(start) {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' => quote = Some(c),
                b'>' => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Given `from` just after an opening `<div ...>`, returns
/// `(start_of_matching_close_tag, index_after_it)`.
fn find_matching_div_close(html: &str, from: usize) -> Option<(usize, usize)> {
    let b = html.as_bytes();
    let mut depth = 1usize;
    let mut i = from;
    while i < b.len() {
        if b[i] == b'<' {
            if is_div_open(b, i) {
                let end = find_tag_end(b, i)?;
                if b[end - 1] != b'/' {
                    depth += 1;
                }
                i = end + 1;
                continue;
            }
            if is_div_close(b, i) {
                let end = find_tag_end(b, i)?;
                depth -= 1;
                if depth == 0 {
                    return Some((i, end + 1));
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    None
}

fn is_excluded_tag(tag: &str) -> bool {
    EXCLUDED_TAG_MARKERS.iter().any(|m| tag.contains(m))
}

/// Removes every excluded `<div>` sub-tree (header, ads, footer...).
fn strip_excluded_blocks(inner: &str) -> String {
    let b = inner.as_bytes();
    let mut out = String::with_capacity(inner.len());
    let mut copy_from = 0usize;
    let mut i = 0usize;

    while i < b.len() {
        if b[i] == b'<' && is_div_open(b, i) {
            let Some(end) = find_tag_end(b, i) else { break };
            if is_excluded_tag(&inner[i..=end]) {
                if let Some((_, after)) = find_matching_div_close(inner, end + 1) {
                    out.push_str(&inner[copy_from..i]);
                    copy_from = after;
                    i = after;
                    continue;
                }
            }
            i = end + 1;
            continue;
        }
        i += 1;
    }
    out.push_str(&inner[copy_from..]);
    out
}

fn html_to_text(html: &str) -> String {
    // Raw newlines in HTML source are plain whitespace; only <br> breaks a line.
    let html = html.replace(['\r', '\n'], " ");
    let s = SCRIPT_RE.replace_all(&html, "");
    let s = STYLE_RE.replace_all(&s, "");
    let s = BR_RE.replace_all(&s, "\n");
    let s = TAG_RE.replace_all(&s, "");
    let decoded = html_escape::decode_html_entities(&s);

    // Trim every line and collapse runs of blank lines to a single one.
    let mut out: Vec<&str> = Vec::new();
    let mut prev_blank = true; // also drops leading blank lines
    for line in decoded.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !prev_blank {
                out.push("");
            }
            prev_blank = true;
        } else {
            out.push(line);
            prev_blank = false;
        }
    }
    while out.last() == Some(&"") {
        out.pop();
    }
    out.join("\n")
}

/// Safety net for when Genius changes its markup and the header leaks into
/// the text ("133 ContributorsTranslations... Read More [Intro] ...").
pub fn strip_header_artifacts(text: &str) -> String {
    if HEADER_RE.is_match(text) {
        if let Some(pos) = text.find("Read More") {
            if pos < 4000 {
                return text[pos + "Read More".len()..].trim().to_string();
            }
        }
        if let Some(pos) = text.find('[') {
            if pos < 4000 {
                return text[pos..].trim().to_string();
            }
        }
    }
    text.to_string()
}

pub fn extract_genius_lyrics(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut blocks: Vec<String> = Vec::new();
    let mut search_idx = 0usize;

    while let Some(rel) = html[search_idx..].find(CONTAINER_ATTR) {
        let attr_pos = search_idx + rel;
        search_idx = attr_pos + CONTAINER_ATTR.len();

        // Opening tag containing the attribute.
        let Some(tag_start) = html[..attr_pos].rfind('<') else {
            continue;
        };
        if !is_div_open(bytes, tag_start) {
            continue;
        }
        let Some(tag_end) = find_tag_end(bytes, tag_start) else {
            continue;
        };
        let content_start = tag_end + 1;
        let Some((content_end, after)) = find_matching_div_close(html, content_start) else {
            continue;
        };

        let inner = strip_excluded_blocks(&html[content_start..content_end]);
        let text = strip_header_artifacts(&html_to_text(&inner));
        if !text.is_empty() {
            blocks.push(text);
        }
        search_idx = after;
    }

    (!blocks.is_empty()).then(|| blocks.join("\n\n"))
}

// ---------------------------------------------------------------------------
// Search + fetch
// ---------------------------------------------------------------------------

/// Picks the first search hit that is a song page (not an artist/album page)
/// on genius.com whose artist plausibly matches.
pub fn pick_song_url(search_json: &Value, artist: &str, title: &str) -> Option<String> {
    let sections = search_json.pointer("/response/sections")?.as_array()?;
    for section in sections {
        let sec_type = section.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if sec_type != "song" && sec_type != "top_hits" {
            continue;
        }
        let Some(hits) = section.get("hits").and_then(|h| h.as_array()) else {
            continue;
        };
        for hit in hits {
            if hit.get("type").and_then(|t| t.as_str()).is_some_and(|t| t != "song") {
                continue;
            }
            let Some(result) = hit.get("result") else {
                continue;
            };
            let Some(url) = result.get("url").and_then(|u| u.as_str()) else {
                continue;
            };
            if !is_trusted_https_url(url, GENIUS_HOSTS) || !url.ends_with("-lyrics") {
                continue;
            }
            let found_artist = result
                .pointer("/primary_artist/name")
                .and_then(|n| n.as_str())
                .or_else(|| result.get("artist_names").and_then(|n| n.as_str()));
            if let Some(fa) = found_artist {
                if !artist_matches(artist, fa) {
                    continue;
                }
            }
            if let Some(ft) = result.get("title").and_then(|t| t.as_str()) {
                if !title_matches(title, ft) {
                    continue;
                }
            }
            return Some(url.to_string());
        }
    }
    None
}

pub async fn fetch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<LyricsSource, String> {
    let search_url = build_url(
        "https://genius.com/api/search/multi",
        &[("q", &format!("{artist} {title}"))],
    );
    let search_json: Value = get_json(client, &search_url, BROWSER_USER_AGENT)
        .await
        .map_err(|e| format!("Genius search failed: {e}"))?;

    let song_url = pick_song_url(&search_json, artist, title)
        .ok_or_else(|| "Genius: no song match found".to_string())?;

    let html = get_text(client, &song_url, BROWSER_USER_AGENT)
        .await
        .map_err(|e| format!("Genius: failed to fetch song page: {e}"))?;

    let lyrics = extract_genius_lyrics(&html)
        .filter(|l| is_acceptable(l))
        .ok_or_else(|| "Genius: lyrics container not found".to_string())?;

    Ok(LyricsSource {
        lyrics,
        source: song_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Realistic first container: header chrome + lyrics + inline ad + annotations.
    const IRON_MAN_HTML: &str = r#"<html><body>
<div class="Lyrics__Container-sc-1ynbvzw-1 kUgSbL" data-lyrics-container="true"><div class="LyricsHeader__Container-sc-1ynbvzw-5 Dzxov" data-exclude-from-selection="true"><div class="HeaderArtistAndTracklistdesktop"><span>133 Contributors</span><button>Translations</button><a href="/de">Deutsch</a><a href="/pt">Português</a><a href="/ru">Русский (Russian)</a></div><h2 class="TextLabel">Iron Man Lyrics</h2><div class="LyricsHeader__Description"><div class="SongDescription"><p>Black Sabbath’s “Iron Man” is widely considered one of the most famous metal songs of all time, along side other Black Sabbath songs such as: “War Pigs” and “Paranoid.” In regard to the song’s signature… <button>Read More</button></p></div></div></div>[Intro]<br/>I am Iron Man<br/><br/>[Verse 1]<br/>Has he lost his mind?<br/><a href="/123"><span class="ReferentFragment">Can he see or is he blind?</span></a><br/>Can he walk at night &amp; day?<div class="InreadContainer__Container-sc-x" data-exclude-from-selection="true"><div>ADVERTISEMENT</div></div><br/>[Outro]</div>
<div class="Lyrics__Container-sc-1ynbvzw-1 kUgSbL" data-lyrics-container="true">Second block line 1<br/>Second block line 2</div>
<div class="Lyrics__Footer"><div>You might also like</div></div>
</body></html>"#;

    #[test]
    fn header_chrome_is_removed_and_lyrics_survive() {
        let out = extract_genius_lyrics(IRON_MAN_HTML).expect("lyrics");
        for artifact in [
            "Contributors",
            "Translations",
            "Deutsch",
            "Русский",
            "Read More",
            "Black Sabbath",
            "Iron Man Lyrics",
            "ADVERTISEMENT",
            "You might also like",
        ] {
            assert!(!out.contains(artifact), "artifact {artifact:?} leaked:\n{out}");
        }
        assert!(
            out.starts_with("[Intro]\nI am Iron Man\n\n[Verse 1]\nHas he lost his mind?"),
            "unexpected start:\n{out}"
        );
        assert!(out.contains("Can he see or is he blind?"));
        assert!(out.contains("Can he walk at night & day?"));
        assert!(out.contains("[Outro]"));
        assert!(out.ends_with("Second block line 1\nSecond block line 2"));
    }

    #[test]
    fn output_is_consistent_no_html_no_edge_whitespace() {
        let out = extract_genius_lyrics(IRON_MAN_HTML).unwrap();
        assert!(!out.contains('<') && !out.contains('>'), "markup leaked:\n{out}");
        assert!(!out.contains("&amp;") && !out.contains('\u{a0}'));
        assert!(!out.contains("\n\n\n"), "more than one blank line in a row");
        assert_eq!(out, out.trim());
        for line in out.lines() {
            assert_eq!(line, line.trim(), "line has edge whitespace: {line:?}");
        }
        // Blocks are separated by exactly one blank line.
        assert!(out.contains("[Outro]\n\nSecond block"));
    }

    #[test]
    fn extraction_is_idempotent_across_reformatting() {
        // Same lyrics with different whitespace/indentation/br styles -> same text.
        let a = r#"<div data-lyrics-container="true">Line one<br/>Line two<br>Line three</div>"#;
        let b = "<div data-lyrics-container=\"true\">\n   Line one<BR />\n\tLine two <br>\r\n  Line three  \n</div>";
        assert_eq!(extract_genius_lyrics(a), extract_genius_lyrics(b));
        assert_eq!(
            extract_genius_lyrics(a).unwrap(),
            "Line one\nLine two\nLine three"
        );
    }

    #[test]
    fn nested_divs_inside_lyrics_do_not_end_container_early() {
        let html = r#"<div data-lyrics-container="true">A<div class="x">B<div>C</div></div>D<br/>E</div><div>outside</div>"#;
        let out = extract_genius_lyrics(html).unwrap();
        assert!(out.contains('A') && out.contains('D') && out.contains('E'));
        assert!(!out.contains("outside"));
    }

    #[test]
    fn entities_are_decoded_once_and_literal_angle_text_is_kept() {
        let html = r#"<div data-lyrics-container="true">Rock &amp; Roll &#x27;n&#39; blues<br/>1 &lt; 2 &gt; 0<br/>caf&eacute;</div>"#;
        let out = extract_genius_lyrics(html).unwrap();
        assert_eq!(out, "Rock & Roll 'n' blues\n1 < 2 > 0\ncafé");
    }

    #[test]
    fn script_and_style_content_is_dropped() {
        let html = r#"<div data-lyrics-container="true">Hi<script>var x = "evil";</script><style>.a{}</style><br/>There</div>"#;
        assert_eq!(extract_genius_lyrics(html).unwrap(), "Hi\nThere");
    }

    #[test]
    fn multibyte_text_is_preserved() {
        let html = r#"<div>prólogo ó 🎵</div><div data-lyrics-container="true">Canción ó ’ apostrophe<br/>Привет<br>🎸 final</div>"#;
        assert_eq!(
            extract_genius_lyrics(html).unwrap(),
            "Canción ó ’ apostrophe\nПривет\n🎸 final"
        );
    }

    #[test]
    fn missing_or_malformed_containers_return_none() {
        assert_eq!(extract_genius_lyrics("<html>no lyrics</html>"), None);
        assert_eq!(extract_genius_lyrics(""), None);
        // Unclosed container, and attribute outside any tag.
        assert_eq!(
            extract_genius_lyrics(r#"<div data-lyrics-container="true">never closed"#),
            None
        );
        assert_eq!(extract_genius_lyrics(r#"text data-lyrics-container="true""#), None);
        // Empty container.
        assert_eq!(
            extract_genius_lyrics(r#"<div data-lyrics-container="true">  <br/> </div>"#),
            None
        );
    }

    #[test]
    fn header_fallback_strips_leaked_header_text() {
        let leaked = "133 ContributorsTranslationsDeutschPortuguêsРусский (Russian)Iron Man LyricsBlack Sabbath’s “Iron Man” is widely considered one of the most famous metal songs… Read More [Intro]\nI am Iron Man";
        assert_eq!(strip_header_artifacts(leaked), "[Intro]\nI am Iron Man");
        // Lyrics that merely mention contributors are untouched.
        let normal = "[Verse]\nThanks to all contributors";
        assert_eq!(strip_header_artifacts(normal), normal);
        assert_eq!(
            strip_header_artifacts("1.2K Contributors\n[Intro]\nHey"),
            "[Intro]\nHey"
        );
    }

    #[test]
    fn header_fallback_works_even_if_markup_exclusion_misses_it() {
        // Header NOT flagged with any exclusion marker/class: text-level fallback catches it.
        let html = r#"<div data-lyrics-container="true"><div class="Foo">133 Contributors<span>Translations</span><p>Song blurb… Read More</p></div>[Intro]<br/>I am Iron Man</div>"#;
        let out = extract_genius_lyrics(html).unwrap();
        assert!(out.starts_with("[Intro]"), "got: {out}");
        assert!(!out.contains("Contributors"));
    }

    #[test]
    fn excluded_div_with_gt_inside_quoted_attribute_is_handled() {
        let html = r#"<div data-lyrics-container="true">keep<div data-foo="a>b" data-exclude-from-selection="true">drop</div>this</div>"#;
        assert_eq!(extract_genius_lyrics(html).unwrap(), "keepthis");
    }

    // ---- search result selection ----

    fn search(hits: Value) -> Value {
        json!({"response": {"sections": [{"type": "top_hits", "hits": hits}]}})
    }

    #[test]
    fn pick_song_url_skips_non_song_hits_and_wrong_hosts() {
        let v = search(json!([
            {"type": "artist", "result": {"url": "https://genius.com/artists/Black-sabbath"}},
            {"type": "song", "result": {"url": "http://genius.com/a-lyrics", "primary_artist": {"name": "Black Sabbath"}}},
            {"type": "song", "result": {"url": "https://evil.com/b-lyrics", "primary_artist": {"name": "Black Sabbath"}}},
            {"type": "song", "result": {"url": "https://genius.com/Black-sabbath-iron-man-lyrics", "primary_artist": {"name": "Black Sabbath"}}},
        ]));
        assert_eq!(
            pick_song_url(&v, "Black Sabbath", "Iron Man").as_deref(),
            Some("https://genius.com/Black-sabbath-iron-man-lyrics")
        );
    }

    #[test]
    fn pick_song_url_rejects_other_artists_but_tolerates_variants() {
        let other = search(json!([
            {"type": "song", "result": {"url": "https://genius.com/x-lyrics", "primary_artist": {"name": "Totally Different"}}}
        ]));
        assert_eq!(pick_song_url(&other, "Black Sabbath", "Iron Man"), None);

        let variant = search(json!([
            {"type": "song", "result": {"url": "https://genius.com/y-lyrics", "primary_artist": {"name": "Beyoncé"}}}
        ]));
        assert!(pick_song_url(&variant, "Beyonce", "Halo").is_some());
        assert!(pick_song_url(&json!({}), "x", "y").is_none());
    }
}
