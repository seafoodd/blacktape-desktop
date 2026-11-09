//! Ranking, normalisation and dedup of merged search results.
//!
//! OWNER: part 2 (ranking). Nobody else edits this file.
//! Contract: `rank` has the SAME output contract as `search::process_raw_results`:
//! returns a single Vec ordered for display, deduplicated, containing no `ItemType::Unknown` items,
//! with the "champion" (best item overall) first, then up to 3 tracks, up to 2 albums, then the remainder
//! sorted by score. Input may contain items from any platform (Bandcamp + Youtube). It must not drop
//! an item type entirely when items of that type exist (after dedup).
//! See C:\Dev\blacktape-search-work\DECISIONS.md (D9-D12).

#![allow(dead_code)]

use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// Tracks / albums placed right after the champion (same slot structure as the legacy feed).
const TRACK_SLOTS: usize = 3;
const ALBUM_SLOTS: usize = 2;

/// Match tiers, index = tier - 1: inside-query, contains, prefix, exact.
/// The artist/band field outweighs the title field at every tier: a query that names an artist
/// ("daft punk") should prefer that artist's own tracks over titles that merely contain the name.
const TITLE_TIERS: [f32; 4] = [25.0, 45.0, 65.0, 85.0];
const BAND_TIERS: [f32; 4] = [30.0, 55.0, 85.0, 115.0];
const TIER_INSIDE: u8 = 1;
const TIER_EXACT: u8 = 4;

/// Placeholder band name YouTube Music uses on artist cards. Never a real credit, never scored.
const PLACEHOLDER_ARTIST: &str = "Artist";

/// Fuzzy (Jaro-Winkler) fallback when no token tier matched.
const FUZZY_THRESHOLD: f64 = 0.85;
const FUZZY_MAX: f32 = 30.0;

/// Share of query tokens found in title or band, scaled to this weight.
const COVERAGE_WEIGHT: f32 = 40.0;
/// Query tokens split between title and band ("daft punk one more time" -> One More Time by Daft Punk).
const SPLIT_BONUS: f32 = 30.0;
/// Artist items whose name equals the query exactly. Large enough that the artist card outscores the
/// same artist's tracks whose title merely starts with the name (band exact + title prefix ~ 177).
const ARTIST_EXACT_BONUS: f32 = 50.0;
/// The weaker of title/band tiers adds this fraction of its score.
const SECONDARY_FIELD_WEIGHT: f32 = 0.25;
/// Title "<artist> - <song>" where <artist> is named in the query but is not this item's band
/// ("Boards of Canada - Roygbiv" by "Our New Band", a bootleg). Subtracted from the score.
const EMBEDDED_ARTIST_PENALTY: f32 = 40.0;

/// Position within the source list (earlier = better). Kept small so it never outweighs a tier.
const POSITION_WEIGHT: f32 = 6.0;
/// Popularity: each of view count and subscriber count adds at most this much.
const POPULARITY_MAX: f32 = 4.0;
/// ln(1e9): a count of one billion is treated as fully popular.
const POPULARITY_LOG_CEIL: f64 = 20.723_266;

/// Tracks shorter or longer than this (seconds) are penalised.
const DURATION_MIN_SECS: u32 = 45;
const DURATION_MAX_SECS: u32 = 900;
const DURATION_PENALTY: f32 = 25.0;

/// Small tie-break for Bandcamp (higher-quality source), replaces the legacy hard +25.
const BANDCAMP_BIAS: f32 = 1.0;

/// Version labels that only mark a re-issue of the same recording (not a different song).
/// Matched as whole normalised tokens, never as substrings ("Monolith Remix" is not "mono").
const NOISE_PHRASES: &[&[&str]] = &[
    &["remaster"],
    &["remastered"],
    &["remastering"],
    &["radio", "edit"],
    &["single", "version"],
    &["album", "version"],
    &["explicit"],
    &["official"],
    &["lyric"],
    &["lyrics"],
    &["music", "video"],
    &["deluxe"],
    &["mono"],
    &["stereo"],
    &["bonus", "track"],
];

/// Entry point used by wiring. Scores every known item, dedups across platforms (best score wins),
/// then lays out: champion, up to 3 tracks, up to 2 albums, remainder by score. Pure and deterministic.
/// Champion: the best item, except that an artist card only takes it when its name equals the query
/// exactly and no track/album is a strong match (exact title or split query), see `champion_index`.
pub fn rank(raw: Vec<SearchSuggestion>, query: &str) -> Vec<SearchSuggestion> {
    // 1. Score each known item. Native index = position within its (platform, type) source list.
    let mut counters = [[0usize; 3]; 4];
    let mut scored: Vec<(f32, usize, SearchSuggestion)> = Vec::new();
    for item in raw {
        let type_slot = match item.item_type {
            ItemType::Track => 0,
            ItemType::Album => 1,
            ItemType::Artist => 2,
            ItemType::Unknown => continue,
        };
        let platform_slot = match item.platform {
            Platform::Youtube => 0,
            Platform::Bandcamp => 1,
            Platform::Soundcloud => 2,
            Platform::Local => 3,
        };
        let native = counters[platform_slot][type_slot];
        counters[platform_slot][type_slot] += 1;
        let s = score(&item, query, native, type_bias(&item.item_type));
        let seq = scored.len();
        scored.push((s, seq, item));
    }

    // 2. Best first; ties keep input order so the output is deterministic.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    // 3. Cross-platform dedup: the first (best scored) occurrence of each key wins.
    let mut seen = HashSet::new();
    let mut pending: Vec<Option<SearchSuggestion>> = scored
        .into_iter()
        .filter(|(_, _, item)| seen.insert(dedup_key(item)))
        .map(|(_, _, item)| Some(item))
        .collect();

    // 4. Champion first, then slot fills (tracks, then albums; champion counts), then the rest.
    let q = tokenize(query);
    let mut out = Vec::with_capacity(pending.len());
    if let Some(ci) = champion_index(&pending, &q) {
        if let Some(champion) = pending[ci].take() {
            out.push(champion);
        }
    }
    for (kind, cap) in [(ItemType::Track, TRACK_SLOTS), (ItemType::Album, ALBUM_SLOTS)] {
        let have = out.iter().filter(|i| i.item_type == kind).count();
        take_matching(&mut pending, &mut out, &kind, cap.saturating_sub(have));
    }
    out.extend(pending.into_iter().flatten());
    out
}

/// Flat ranking for the full search page (v2, D4): same per-item `score` and the same cross-platform
/// dedup as `rank`, but NO champion / slot layout - the result is simply every known item sorted by
/// score descending (ties keep input order). `ItemType::Unknown` items are dropped. Intended for a list of
/// ONE item type from one or more platforms (the page ranks tracks, albums and artists separately), but
/// mixed input is accepted. Pure and deterministic. Part A may tune `score`; this signature is frozen.
pub fn rank_flat(raw: Vec<SearchSuggestion>, query: &str) -> Vec<SearchSuggestion> {
    let mut counters = [[0usize; 3]; 4];
    let mut scored: Vec<(f32, usize, SearchSuggestion)> = Vec::new();
    for item in raw {
        let type_slot = match item.item_type {
            ItemType::Track => 0,
            ItemType::Album => 1,
            ItemType::Artist => 2,
            ItemType::Unknown => continue,
        };
        let platform_slot = match item.platform {
            Platform::Youtube => 0,
            Platform::Bandcamp => 1,
            Platform::Soundcloud => 2,
            Platform::Local => 3,
        };
        let native = counters[platform_slot][type_slot];
        counters[platform_slot][type_slot] += 1;
        let s = score(&item, query, native, type_bias(&item.item_type));
        let seq = scored.len();
        scored.push((s, seq, item));
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut seen = HashSet::new();
    scored
        .into_iter()
        .filter(|(_, _, item)| seen.insert(dedup_key(item)))
        .map(|(_, _, item)| item)
        .collect()
}

/// Moves up to `limit` items of `kind` from `pending` (in order) into `out`.
fn take_matching(
    pending: &mut [Option<SearchSuggestion>],
    out: &mut Vec<SearchSuggestion>,
    kind: &ItemType,
    limit: usize,
) {
    let mut taken = 0;
    for slot in pending.iter_mut() {
        if taken >= limit {
            break;
        }
        if matches!(slot, Some(item) if item.item_type == *kind) {
            if let Some(item) = slot.take() {
                out.push(item);
                taken += 1;
            }
        }
    }
}

/// Normalises text for comparison. Pure.
/// Unicode NFKD -> drop combining marks (diacritics: "Beyonce\u{301}" == "beyonce"), lowercase (Unicode),
/// "&" -> "and", apostrophes removed ("Don't" -> "dont"), other punctuation/symbols become spaces
/// (keeps letters/digits of ANY script, e.g. Cyrillic, CJK), whitespace collapsed and trimmed.
/// "  Sigur   Ros!! " -> "sigur ros". Empty/whitespace/symbols only -> "".
pub fn normalize(s: &str) -> String {
    let mut buf = String::with_capacity(s.len());
    for c in s.nfkd() {
        if is_diacritic_mark(c) {
            continue;
        }
        if c == '&' {
            buf.push_str(" and ");
        } else if is_apostrophe(c) {
            // Dropped without a space so contractions stay one word.
        } else if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                if !is_diacritic_mark(lc) {
                    buf.push(lc);
                }
            }
        } else {
            buf.push(' ');
        }
    }
    words(&buf).join(" ")
}

/// Splits normalised text into tokens (whitespace split of `normalize`). Strips featuring clauses first:
/// "(feat. X)", "[ft. X]", " feat. X", " ft X", " featuring X" and everything after them are removed,
/// so "Song (feat. Foo)" tokenises like "Song". Pure.
pub fn tokenize(s: &str) -> Vec<String> {
    words(&normalize(strip_featuring(s)))
}

/// Stable dedup key for cross-platform duplicate detection. Pure.
/// Format: "{type}:{title_core}:{primary_artist}" where title_core = normalize(title) with trailing
/// version noise removed: parenthesised/bracketed suffixes that are only noise, " - remastered ...",
/// " - radio edit", " - single version", feat. clauses; primary_artist = normalize(first artist, split on
/// ',' '&' 'feat'). Same song on Bandcamp and YouTube must produce the same key; real remixes keep their
/// remix name: "Song (Foo Remix)" != "Song". Artist items: key uses name only.
pub fn dedup_key(item: &SearchSuggestion) -> String {
    let kind = type_label(&item.item_type);
    let title = core_title(&item.name);
    match item.item_type {
        ItemType::Artist => format!("{kind}:{title}:"),
        _ => format!("{kind}:{title}:{}", primary_artist(&item.band_name)),
    }
}

/// Relevance score of one item for the query (higher = better). Pure and deterministic, no NaN.
/// `native_index` = position within its source list (earlier = better), `type_bias` per item type.
/// Must (tested): exact title > prefix > substring > unrelated; exact artist-name match for Artist items
/// beats a track merely containing the name; query tokens split across title+artist both count
/// ("daft punk one more time" finds track "One More Time" by "Daft Punk"); diacritics-insensitive;
/// feat. clauses ignored; popularity (view/subscriber counts) is a bounded, logarithmic tiebreaker that can
/// never outweigh an exact match; very short (<45s) or very long (>900s) tracks are penalised.
pub fn score(
    item: &SearchSuggestion,
    query: &str,
    native_index: usize,
    type_bias: f32,
) -> f32 {
    let q = tokenize(query);
    let is_artist = item.item_type == ItemType::Artist;
    let f = fields(item, &q);

    let (title_s, title_tier) = field_score(&f.title, &q, &TITLE_TIERS);
    let (band_s, _) = field_score(&f.band, &q, &BAND_TIERS);

    // The stronger field carries the score; the weaker one adds a little, so a track whose band
    // matches exactly does not stack up with a title that merely contains the query.
    let (hi, lo) = if title_s >= band_s {
        (title_s, band_s)
    } else {
        (band_s, title_s)
    };
    let mut s = hi + SECONDARY_FIELD_WEIGHT * lo;

    if !q.is_empty() {
        let matched = q
            .iter()
            .filter(|t| f.title.contains(t) || f.band.contains(t))
            .count();
        s += COVERAGE_WEIGHT * matched as f32 / q.len() as f32;

        if split_match(&f, &q) {
            s += SPLIT_BONUS;
        }
    }

    if is_artist && title_tier == TIER_EXACT {
        s += ARTIST_EXACT_BONUS;
    }

    if f.embedded_artist {
        s -= EMBEDDED_ARTIST_PENALTY;
    }

    let bias = if type_bias.is_finite() { type_bias } else { 0.0 };
    let position = POSITION_WEIGHT / (native_index as f32 + 1.0).sqrt();
    s += position + bias;

    if item.platform == Platform::Bandcamp {
        s += BANDCAMP_BIAS;
    }

    s += popularity(item.view_count) + popularity(item.subscriber_count);

    if item.item_type == ItemType::Track {
        if let Some(d) = item.duration {
            if d < DURATION_MIN_SECS || d > DURATION_MAX_SECS {
                s -= DURATION_PENALTY;
            }
        }
    }

    s
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Match-ready token lists of one item's text fields.
struct Fields {
    title: Vec<String>,
    /// Empty for artist items and for the placeholder band "Artist".
    band: Vec<String>,
    /// The title opens with "<artist> - " where the artist is named in the query and is not the band.
    embedded_artist: bool,
}

fn fields(item: &SearchSuggestion, q: &[String]) -> Fields {
    let title = words(&core_title(&item.name));
    if item.item_type == ItemType::Artist {
        return Fields {
            title,
            band: Vec::new(),
            embedded_artist: false,
        };
    }
    let raw_band = item.band_name.trim();
    let band = if raw_band.eq_ignore_ascii_case(PLACEHOLDER_ARTIST) {
        Vec::new()
    } else {
        words(&normalize(strip_featuring(raw_band)))
    };
    let embedded_artist = embedded_artist_prefix(&item.name, &band, q);
    Fields {
        title,
        band,
        embedded_artist,
    }
}

/// True when the raw name starts with "<words> - " and those words are all in the query but do not
/// belong to the band. Needs a known band (an unknown band cannot be contradicted).
/// A part after " - " that is only a version label ("Radio Edit", "Remastered 2011", "Live ...") is
/// not an artist prefix: "Around the World - Radio Edit" by Daft Punk is still the official track.
fn embedded_artist_prefix(name: &str, band: &[String], q: &[String]) -> bool {
    if band.is_empty() || q.is_empty() {
        return false;
    }
    let no_feat = strip_featuring(name);
    let Some(p) = no_feat.find(" - ") else {
        return false;
    };
    let rest = &no_feat[p + 3..];
    if is_version_label(rest) {
        return false;
    }
    let left = words(&normalize(&no_feat[..p]));
    if left.is_empty() || phrase_tier(band, &left) > 0 {
        return false;
    }
    left.iter().all(|t| q.contains(t))
}

/// Version or live label that may follow " - " in a title: known noise ("Radio Edit", "Remastered
/// 2011", "Single Version") or a live label starting with "live" ("Live", "Live at Wembley").
fn is_version_label(rest: &str) -> bool {
    if is_noise_label(rest) {
        return true;
    }
    words(&normalize(rest)).first().map_or(false, |w| w == "live")
}

/// Query tokens found in the title AND, separately, in the band ("daft punk one more time" against
/// "One More Time" by "Daft Punk"): the query is split between the two fields.
fn split_match(f: &Fields, q: &[String]) -> bool {
    let title_only = q.iter().any(|t| f.title.contains(t) && !f.band.contains(t));
    let band_only = q.iter().any(|t| f.band.contains(t) && !f.title.contains(t));
    title_only && band_only
}

/// Strong non-artist match: title equal to the query, or the query split across title and band.
fn is_strong_match(item: &SearchSuggestion, q: &[String]) -> bool {
    if q.is_empty() || item.item_type == ItemType::Artist {
        return false;
    }
    let f = fields(item, q);
    phrase_tier(&f.title, q) == TIER_EXACT || split_match(&f, q)
}

/// Index in `pending` (score order, all present) of the item that takes the champion slot.
/// An artist card keeps the slot only when its name equals the query exactly AND no track or album
/// is a strong match (exact title or split query). Otherwise the best track/album takes it.
/// Empty query: the best item stays first.
fn champion_index(pending: &[Option<SearchSuggestion>], q: &[String]) -> Option<usize> {
    let first = pending.iter().position(Option::is_some)?;
    let top = pending[first].as_ref()?;
    if q.is_empty() || top.item_type != ItemType::Artist {
        return Some(first);
    }
    let artist_exact = phrase_tier(&fields(top, q).title, q) == TIER_EXACT;
    let strong_other = pending
        .iter()
        .flatten()
        .any(|i| is_strong_match(i, q));
    if artist_exact && !strong_other {
        return Some(first);
    }
    Some(
        pending
            .iter()
            .position(|s| matches!(s, Some(i) if i.item_type != ItemType::Artist))
            .unwrap_or(first),
    )
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// Combining marks left after NFKD that belong to Latin-style diacritics (and kana voicing marks).
/// Other marks (e.g. Indic vowel signs) are kept so those words stay intact.
fn is_diacritic_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F
            | 0x3099
            | 0x309A
    )
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '`' | '\u{2018}' | '\u{2019}' | '\u{02BC}')
}

/// Removes a featuring clause and everything after it. Matching is ASCII case-insensitive and
/// requires "feat"/"ft"/"featuring" to be a whole word, so "Featherweight" or "Soft Cell" are untouched.
fn strip_featuring(s: &str) -> &str {
    // ASCII lowercasing keeps byte offsets identical to `s`.
    let lower = s.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b != b'(' && b != b'[' && b != b' ' {
            continue;
        }
        let rest = &lower[i + 1..];
        for kw in ["featuring", "feat", "ft"] {
            if let Some(after) = rest.strip_prefix(kw) {
                let next_is_letter = after
                    .as_bytes()
                    .first()
                    .map_or(false, |c| c.is_ascii_alphabetic());
                if !next_is_letter {
                    return &s[..i];
                }
            }
        }
    }
    s
}

fn is_noise_label(label: &str) -> bool {
    let tokens = words(&normalize(label));
    NOISE_PHRASES.iter().any(|phrase| {
        tokens
            .windows(phrase.len())
            .any(|window| window.iter().zip(phrase.iter()).all(|(token, word)| token == word))
    })
}

/// Byte index where a trailing noise suffix starts, if the title ends with one.
fn noise_cut_point(t: &str) -> Option<usize> {
    if t.ends_with(')') || t.ends_with(']') {
        let open = t.rfind(|c: char| c == '(' || c == '[')?;
        let inner = &t[open + 1..t.len() - 1];
        return if is_noise_label(inner) { Some(open) } else { None };
    }
    let p = t.rfind(" - ")?;
    if is_noise_label(&t[p + 3..]) {
        Some(p)
    } else {
        None
    }
}

/// Lowercased title with trailing version-noise suffixes removed, repeatedly.
/// "Song (Foo Remix) - Remastered 2011" -> "song (foo remix)"; "Song (Live)" is kept.
fn strip_version_noise(name: &str) -> String {
    let mut current = name.to_lowercase();
    loop {
        let trimmed = current.trim();
        match noise_cut_point(trimmed) {
            Some(cut) => current = trimmed[..cut].to_string(),
            None => return trimmed.to_string(),
        }
    }
}

/// Normalised title used for matching and dedup: no featuring clause, no version noise.
/// Falls back to the featuring-stripped title, then the raw title, when cleaning empties it.
fn core_title(name: &str) -> String {
    let no_feat = strip_featuring(name);
    let core = normalize(&strip_version_noise(no_feat));
    if !core.is_empty() {
        return core;
    }
    let plain = normalize(no_feat);
    if !plain.is_empty() {
        plain
    } else {
        normalize(name)
    }
}

/// Normalised first artist of a (possibly multi-artist) band string.
fn primary_artist(band: &str) -> String {
    let first = band
        .split(|c: char| c == ',' || c == '&')
        .next()
        .unwrap_or("");
    normalize(strip_featuring(first))
}

fn type_label(t: &ItemType) -> &'static str {
    match t {
        ItemType::Album => "album",
        ItemType::Artist => "artist",
        ItemType::Track => "track",
        ItemType::Unknown => "unknown",
    }
}

fn type_bias(t: &ItemType) -> f32 {
    match t {
        ItemType::Artist => 5.0,
        ItemType::Album => 2.0,
        ItemType::Track | ItemType::Unknown => 0.0,
    }
}

/// Phrase match of `needle` against `hay` (both token lists).
/// 4 = equal, 3 = hay starts with needle, 2 = needle inside hay, 1 = hay inside needle, 0 = none.
fn phrase_tier(hay: &[String], needle: &[String]) -> u8 {
    if needle.is_empty() || hay.is_empty() {
        return 0;
    }
    if hay == needle {
        return TIER_EXACT;
    }
    if hay.len() > needle.len() {
        if hay.starts_with(needle) {
            return 3;
        }
        if hay.windows(needle.len()).any(|w| w == needle) {
            return 2;
        }
    }
    if needle.len() > hay.len() && needle.windows(hay.len()).any(|w| w == hay) {
        return TIER_INSIDE;
    }
    0
}

/// Score of one text field (title or band) against the query tokens: tier score, or a fuzzy fallback.
/// Returns the score and the phrase tier that matched (0 when only fuzzy or nothing).
fn field_score(tokens: &[String], q: &[String], tiers: &[f32; 4]) -> (f32, u8) {
    if q.is_empty() || tokens.is_empty() {
        return (0.0, 0);
    }
    let tier = phrase_tier(tokens, q);
    if tier > 0 {
        return (tiers[tier as usize - 1], tier);
    }
    let jw = strsim::jaro_winkler(&tokens.join(" "), &q.join(" "));
    if jw > FUZZY_THRESHOLD {
        let ratio = ((jw - FUZZY_THRESHOLD) / (1.0 - FUZZY_THRESHOLD)).min(1.0);
        ((ratio as f32) * FUZZY_MAX, 0)
    } else {
        (0.0, 0)
    }
}

/// Bounded logarithmic popularity in [0, POPULARITY_MAX]. None or zero contributes nothing.
fn popularity(count: Option<u64>) -> f32 {
    match count {
        Some(n) if n > 0 => {
            let ratio = ((n as f64 + 1.0).ln() / POPULARITY_LOG_CEIL).min(1.0);
            (ratio as f32) * POPULARITY_MAX
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(t: ItemType, name: &str, band: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type: t,
            name: name.into(),
            band_name: band.into(),
            album_name: None,
            item_url_path: String::new(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Youtube,
        }
    }

    fn track(name: &str, band: &str) -> SearchSuggestion {
        make(ItemType::Track, name, band)
    }

    fn album(name: &str, band: &str) -> SearchSuggestion {
        make(ItemType::Album, name, band)
    }

    fn artist(name: &str) -> SearchSuggestion {
        make(ItemType::Artist, name, "Artist")
    }

    fn key(t: ItemType, name: &str, band: &str) -> String {
        dedup_key(&make(t, name, band))
    }

    fn names(items: &[SearchSuggestion]) -> Vec<String> {
        items.iter().map(|i| i.name.clone()).collect()
    }

    // ---- normalize ----

    #[test]
    fn normalize_strips_diacritics() {
        assert_eq!(normalize("Beyoncé"), "beyonce");
        assert_eq!(normalize("Beyonce\u{301}"), "beyonce");
        assert_eq!(normalize("Ünïcödé"), "unicode");
        assert_eq!(normalize("Sigur Rós"), "sigur ros");
    }

    #[test]
    fn normalize_lowercases_unicode() {
        assert_eq!(normalize("ÉCOLE"), "ecole");
        assert_eq!(normalize("İstanbul"), "istanbul");
    }

    #[test]
    fn normalize_collapses_whitespace_and_punctuation() {
        assert_eq!(normalize("  Sigur   Ros!! "), "sigur ros");
        assert_eq!(normalize("Mr. Brightside"), "mr brightside");
        assert_eq!(normalize("AC/DC"), "ac dc");
        assert_eq!(normalize("Hello\tWorld\n"), "hello world");
    }

    #[test]
    fn normalize_ampersand_becomes_and() {
        assert_eq!(normalize("Simon & Garfunkel"), "simon and garfunkel");
        assert_eq!(normalize("Rock&Roll"), "rock and roll");
    }

    #[test]
    fn normalize_drops_apostrophes() {
        assert_eq!(normalize("Don't Stop"), "dont stop");
        assert_eq!(normalize("Don\u{2019}t"), "dont");
    }

    #[test]
    fn normalize_keeps_cyrillic_and_cjk() {
        assert_eq!(normalize("Кино"), "кино");
        assert_eq!(normalize("ПЕСНЯ 2"), "песня 2");
        assert_eq!(normalize("東京事変"), "東京事変");
    }

    #[test]
    fn normalize_applies_compatibility_forms() {
        assert_eq!(normalize("ＡＢＣ"), "abc");
        assert_eq!(normalize("ﬁne"), "fine");
    }

    #[test]
    fn normalize_empty_and_symbol_only_inputs() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   \t\n "), "");
        assert_eq!(normalize("!!! ???"), "");
        assert_eq!(normalize("🔥"), "");
    }

    #[test]
    fn normalize_is_idempotent() {
        for s in ["Beyoncé (feat. X) & Y", "  A&B!!  ", "Кино", "", "Don't"] {
            let once = normalize(s);
            assert_eq!(normalize(&once), once, "input {s:?}");
        }
    }

    // ---- tokenize ----

    #[test]
    fn tokenize_plain_words() {
        assert_eq!(tokenize("Hello World"), vec!["hello", "world"]);
    }

    #[test]
    fn tokenize_strips_parenthesised_feat() {
        assert_eq!(tokenize("Song (feat. Foo)"), vec!["song"]);
        assert_eq!(tokenize("Song (ft. Foo Bar)"), vec!["song"]);
        assert_eq!(tokenize("Song (FEAT. Foo)"), vec!["song"]);
    }

    #[test]
    fn tokenize_strips_bracketed_ft() {
        assert_eq!(tokenize("Song [ft. Foo]"), vec!["song"]);
    }

    #[test]
    fn tokenize_strips_bare_feat_clauses() {
        assert_eq!(tokenize("Song ft Foo"), vec!["song"]);
        assert_eq!(tokenize("Song feat Foo"), vec!["song"]);
        assert_eq!(tokenize("Song featuring Foo & Bar"), vec!["song"]);
    }

    #[test]
    fn tokenize_drops_everything_after_featuring() {
        assert_eq!(tokenize("Song (feat. Foo) [Remix]"), vec!["song"]);
    }

    #[test]
    fn tokenize_does_not_cut_words_that_start_with_feat_or_ft() {
        assert_eq!(tokenize("Featherweight"), vec!["featherweight"]);
        assert_eq!(tokenize("Soft Cell"), vec!["soft", "cell"]);
        assert_eq!(tokenize("Lift Off"), vec!["lift", "off"]);
        assert_eq!(tokenize("Shift ftw"), vec!["shift", "ftw"]);
    }

    #[test]
    fn tokenize_unicode_and_diacritics() {
        assert_eq!(tokenize("Beyoncé (feat. Jay-Z)"), vec!["beyonce"]);
        assert_eq!(tokenize("Песня (feat. X)"), vec!["песня"]);
    }

    #[test]
    fn tokenize_empty_and_feat_only() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("   ").is_empty());
        assert!(tokenize("(feat. Nobody)").is_empty());
    }

    // ---- dedup_key ----

    #[test]
    fn dedup_same_song_across_platforms_matches() {
        let bandcamp = key(ItemType::Track, "One More Time", "Daft Punk");
        let youtube = key(ItemType::Track, "One More Time (Radio Edit)", "Daft Punk");
        assert_eq!(bandcamp, youtube);
        assert_eq!(bandcamp, "track:one more time:daft punk");
    }

    #[test]
    fn dedup_remaster_and_radio_edit_suffixes_are_noise() {
        let base = key(ItemType::Track, "Song", "X");
        assert_eq!(key(ItemType::Track, "Song - Remastered 2011", "X"), base);
        assert_eq!(key(ItemType::Track, "Song (Remastered)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song - Radio Edit", "X"), base);
        assert_eq!(key(ItemType::Track, "Song (Single Version)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song [Explicit]", "X"), base);
    }

    #[test]
    fn dedup_feat_clause_is_ignored() {
        assert_eq!(
            key(ItemType::Track, "Song (feat. Foo)", "Bar"),
            key(ItemType::Track, "Song", "Bar")
        );
    }

    #[test]
    fn dedup_real_remix_keeps_its_name() {
        let plain = key(ItemType::Track, "Song", "X");
        let remix = key(ItemType::Track, "Song (Foo Remix)", "X");
        assert_ne!(plain, remix);
        assert!(remix.contains("foo remix"));
        assert_eq!(
            key(ItemType::Track, "Song (Foo Remix) - Remastered", "X"),
            remix
        );
    }

    #[test]
    fn dedup_substring_of_noise_word_is_not_noise() {
        // "Monolith" contains "mono" and "Stereophonics" contains "stereo", but they are not the labels.
        assert_ne!(
            key(ItemType::Track, "Song (Monolith Remix)", "X"),
            key(ItemType::Track, "Song", "X")
        );
        assert_ne!(
            key(ItemType::Track, "Song (Stereophonics Remix)", "X"),
            key(ItemType::Track, "Song", "X")
        );
        assert_ne!(
            key(ItemType::Track, "Song (Monologue)", "X"),
            key(ItemType::Track, "Song", "X")
        );
        assert_ne!(
            key(ItemType::Track, "Song (Lyrical Mix)", "X"),
            key(ItemType::Track, "Song", "X")
        );
    }

    #[test]
    fn dedup_whole_word_noise_labels_match() {
        let base = key(ItemType::Track, "Song", "X");
        assert_eq!(key(ItemType::Track, "Song (Mono)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song (Stereo)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song (Official Video)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song (Lyric Video)", "X"), base);
        assert_eq!(key(ItemType::Track, "Song - Remastered", "X"), base);
    }

    #[test]
    fn dedup_documents_ampersand_and_comma_artist_split() {
        // Documented behaviour: the primary artist is the part before ',' or '&'.
        assert_eq!(
            key(ItemType::Track, "Song", "Tyler, The Creator"),
            "track:song:tyler"
        );
        assert_eq!(
            key(ItemType::Track, "Song", "Simon & Garfunkel"),
            "track:song:simon"
        );
    }

    #[test]
    fn dedup_live_version_is_not_noise() {
        assert_ne!(
            key(ItemType::Track, "Song (Live)", "X"),
            key(ItemType::Track, "Song", "X")
        );
    }

    #[test]
    fn dedup_primary_artist_only() {
        let base = key(ItemType::Track, "Song", "Daft Punk");
        assert_eq!(key(ItemType::Track, "Song", "Daft Punk, Pharrell Williams"), base);
        assert_eq!(key(ItemType::Track, "Song", "Daft Punk feat. Pharrell"), base);
        assert_eq!(key(ItemType::Track, "Song", "daft punk"), base);
        assert_eq!(key(ItemType::Track, "Song", "DAFT PUNK & Friends"), base);
    }

    #[test]
    fn dedup_is_case_and_diacritic_insensitive() {
        assert_eq!(
            key(ItemType::Track, "Кино", "Группа"),
            key(ItemType::Track, "кино", "группа")
        );
        assert_eq!(
            key(ItemType::Album, "Ágætis byrjun", "Sigur Rós"),
            key(ItemType::Album, "agætis byrjun", "SIGUR ROS")
        );
    }

    #[test]
    fn dedup_album_deluxe_edition_is_noise() {
        assert_eq!(
            key(ItemType::Album, "Random Access Memories (Deluxe Edition)", "Daft Punk"),
            key(ItemType::Album, "Random Access Memories", "Daft Punk")
        );
    }

    #[test]
    fn dedup_type_separates_keys() {
        assert_ne!(
            key(ItemType::Track, "Random Access Memories", "Daft Punk"),
            key(ItemType::Album, "Random Access Memories", "Daft Punk")
        );
    }

    #[test]
    fn dedup_artist_uses_name_only() {
        assert_eq!(
            dedup_key(&artist("Daft Punk")),
            dedup_key(&make(ItemType::Artist, "Daft Punk", ""))
        );
        assert_eq!(dedup_key(&artist("Daft Punk")), "artist:daft punk:");
    }

    #[test]
    fn dedup_empty_fields_do_not_panic() {
        assert_eq!(key(ItemType::Track, "", ""), "track::");
        assert_eq!(key(ItemType::Artist, "", ""), "artist::");
    }

    #[test]
    fn dedup_noise_only_title_falls_back_to_raw_words() {
        assert_eq!(key(ItemType::Track, "(Remastered)", "X"), "track:remastered:x");
    }

    // ---- score ----

    #[test]
    fn score_exact_beats_prefix_beats_substring_beats_unrelated() {
        let q = "one more time";
        let exact = score(&track("One More Time", "Daft Punk"), q, 0, 0.0);
        let prefix = score(&track("One More Time Forever", "Someone"), q, 0, 0.0);
        let substring = score(&track("Hello One More Time", "Someone"), q, 0, 0.0);
        let unrelated = score(&track("Something Else", "Nobody"), q, 0, 0.0);
        assert!(exact > prefix, "{exact} > {prefix}");
        assert!(prefix > substring, "{prefix} > {substring}");
        assert!(substring > unrelated, "{substring} > {unrelated}");
    }

    #[test]
    fn score_exact_artist_beats_track_containing_the_name() {
        let q = "daft punk";
        let artist_hit = score(&artist("Daft Punk"), q, 0, 5.0);
        let contains = score(&track("Harder Daft Punk Forever", "Someone"), q, 0, 0.0);
        let same_artist_prefix = score(&track("Daft Punk Is Playing", "Daft Punk"), q, 0, 0.0);
        assert!(artist_hit > contains);
        assert!(artist_hit > same_artist_prefix, "{artist_hit} > {same_artist_prefix}");
    }

    #[test]
    fn score_split_query_finds_the_track() {
        let q = "daft punk one more time";
        let right = score(&track("One More Time", "Daft Punk"), q, 0, 0.0);
        let wrong = score(
            &track("Harder Better Faster Stronger", "Daft Punk"),
            q,
            0,
            0.0,
        );
        let wrong_artist = score(&artist("Daft Punk"), q, 0, 5.0);
        assert!(right > wrong, "{right} > {wrong}");
        assert!(right > wrong_artist, "{right} > {wrong_artist}");
    }

    #[test]
    fn score_is_diacritics_insensitive() {
        assert_eq!(
            score(&track("Beyoncé", "X"), "beyonce", 0, 0.0),
            score(&track("Beyonce", "X"), "beyonce", 0, 0.0)
        );
        assert_eq!(
            score(&track("Beyoncé", "X"), "BEYONCÉ", 0, 0.0),
            score(&track("Beyonce", "X"), "beyonce", 0, 0.0)
        );
    }

    #[test]
    fn score_ignores_feat_clauses_in_title_and_query() {
        let plain = score(&track("Song", "Bar"), "song", 0, 0.0);
        assert_eq!(score(&track("Song (feat. Foo)", "Bar"), "song", 0, 0.0), plain);
        assert_eq!(score(&track("Song", "Bar"), "song (feat. Foo)", 0, 0.0), plain);
    }

    #[test]
    fn score_popularity_cannot_outweigh_exact_match() {
        let exact = score(&track("Song", "X"), "song", 50, 0.0);
        let popular_prefix = score(
            &SearchSuggestion {
                view_count: Some(u64::MAX),
                subscriber_count: Some(u64::MAX),
                audience_kind: None,
                ..track("Song Forever", "X")
            },
            "song",
            0,
            0.0,
        );
        assert!(exact > popular_prefix, "{exact} > {popular_prefix}");
    }

    #[test]
    fn score_popularity_is_bounded() {
        let base = score(&track("Song", "X"), "song", 0, 0.0);
        let huge = score(
            &SearchSuggestion {
                view_count: Some(u64::MAX),
                subscriber_count: Some(u64::MAX),
                audience_kind: None,
                ..track("Song", "X")
            },
            "song",
            0,
            0.0,
        );
        assert!(huge >= base);
        assert!(huge - base <= 2.0 * POPULARITY_MAX + 1e-3, "delta {}", huge - base);
    }

    #[test]
    fn score_popularity_is_monotonic() {
        let with = |v: u64| {
            score(
                &SearchSuggestion {
                    view_count: Some(v),
                    ..track("Song", "X")
                },
                "song",
                0,
                0.0,
            )
        };
        assert!(with(10) < with(10_000));
        assert!(with(10_000) < with(10_000_000));
        assert_eq!(with(0), score(&track("Song", "X"), "song", 0, 0.0));
    }

    #[test]
    fn score_penalises_very_short_and_very_long_tracks() {
        let normal = SearchSuggestion {
            duration: Some(200),
            ..track("Song", "X")
        };
        let short = SearchSuggestion {
            duration: Some(30),
            ..track("Song", "X")
        };
        let long = SearchSuggestion {
            duration: Some(1000),
            ..track("Song", "X")
        };
        let base = score(&normal, "song", 0, 0.0);
        assert!(score(&short, "song", 0, 0.0) < base);
        assert!(score(&long, "song", 0, 0.0) < base);
        assert_eq!(score(&short, "song", 0, 0.0), base - DURATION_PENALTY);
    }

    #[test]
    fn score_duration_penalty_only_applies_to_tracks() {
        let with = SearchSuggestion {
            duration: Some(10),
            ..album("Song", "X")
        };
        assert_eq!(
            score(&with, "song", 0, 0.0),
            score(&album("Song", "X"), "song", 0, 0.0)
        );
    }

    #[test]
    fn score_bandcamp_bias_is_small_and_exact() {
        let yt = track("Song", "X");
        let bc = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Song", "X")
        };
        let diff = score(&bc, "song", 0, 0.0) - score(&yt, "song", 0, 0.0);
        assert!((diff - BANDCAMP_BIAS).abs() < 1e-4, "diff {diff}");
        assert!(diff < 5.0);
    }

    #[test]
    fn score_native_index_prefers_earlier_items() {
        let item = track("Song", "X");
        assert!(score(&item, "song", 0, 0.0) > score(&item, "song", 5, 0.0));
    }

    #[test]
    fn score_type_bias_is_added_verbatim() {
        let item = track("Song", "X");
        let diff = score(&item, "song", 0, 5.0) - score(&item, "song", 0, 0.0);
        assert!((diff - 5.0).abs() < 1e-4, "diff {diff}");
    }

    #[test]
    fn score_is_deterministic() {
        let item = track("One More Time (feat. X)", "Daft Punk");
        assert_eq!(
            score(&item, "daft punk one more time", 3, 2.0),
            score(&item, "daft punk one more time", 3, 2.0)
        );
    }

    #[test]
    fn score_is_finite_for_degenerate_input() {
        let names = ["", " ", "(feat.)", "!!!", "🔥", "Кино", "(Remastered)", "a"];
        let bands = ["", "Artist", "&,", "Daft Punk, "];
        let queries = ["", "   ", "feat.", "a", "daft punk", "!!!"];
        let views = [None, Some(0), Some(u64::MAX)];
        let durations = [None, Some(0), Some(u32::MAX)];
        let biases = [0.0f32, f32::NAN, f32::INFINITY, -5.0];
        let types = [ItemType::Track, ItemType::Album, ItemType::Artist, ItemType::Unknown];
        for t in &types {
            for name in names {
                for band in bands {
                    for q in queries {
                        for v in views {
                            for d in durations {
                                for b in biases {
                                    let item = SearchSuggestion {
                                        view_count: v,
                                        subscriber_count: v,
                                        audience_kind: None,
                                        duration: d,
                                        ..make(t.clone(), name, band)
                                    };
                                    let s = score(&item, q, usize::MAX, b);
                                    assert!(s.is_finite(), "{name:?} {band:?} {q:?} -> {s}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- rank ----

    #[test]
    fn rank_empty_input_returns_empty() {
        assert!(rank(Vec::new(), "anything").is_empty());
        assert!(rank(Vec::new(), "").is_empty());
    }

    #[test]
    fn rank_drops_unknown_items() {
        let out = rank(
            vec![
                make(ItemType::Unknown, "Song", "X"),
                track("Song", "X"),
                make(ItemType::Unknown, "Other", "Y"),
            ],
            "song",
        );
        assert_eq!(out.len(), 1);
        assert!(out.iter().all(|i| i.item_type != ItemType::Unknown));
        assert_eq!(out[0].name, "Song");
    }

    #[test]
    fn rank_all_unknown_returns_empty() {
        assert!(rank(vec![make(ItemType::Unknown, "x", "y")], "x").is_empty());
    }

    #[test]
    fn rank_exact_artist_is_champion_over_popular_unrelated_track() {
        let popular = SearchSuggestion {
            view_count: Some(1_000_000_000),
            ..track("Something Unrelated", "Nobody")
        };
        let artist_hit = SearchSuggestion {
            subscriber_count: Some(5_000_000),
            audience_kind: None,
            ..artist("Daft Punk")
        };
        let out = rank(vec![popular, artist_hit], "daft punk");
        assert_eq!(out[0].item_type, ItemType::Artist);
        assert_eq!(out[0].name, "Daft Punk");
    }

    #[test]
    fn rank_split_query_puts_right_track_first() {
        let out = rank(
            vec![
                track("Harder Better Faster Stronger", "Daft Punk"),
                artist("Daft Punk"),
                track("One More Time", "Daft Punk"),
            ],
            "daft punk one more time",
        );
        assert_eq!(out[0].name, "One More Time");
        assert_eq!(out[0].band_name, "Daft Punk");
    }

    #[test]
    fn rank_collapses_cross_platform_duplicates() {
        let bandcamp = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("One More Time", "Daft Punk")
        };
        let youtube = SearchSuggestion {
            view_count: Some(857_000_000),
            ..track("One More Time (Radio Edit)", "Daft Punk")
        };
        let out = rank(vec![bandcamp, youtube], "one more time");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "One More Time (Radio Edit)");
    }

    #[test]
    fn rank_collapses_same_platform_duplicates() {
        let out = rank(
            vec![
                track("Song", "X"),
                track("Song (Remastered)", "X"),
                track("Song - Radio Edit", "X"),
            ],
            "song",
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn rank_keeps_distinct_items() {
        let out = rank(vec![track("Song A", "X"), track("Song B", "X")], "song");
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rank_caps_track_and_album_slots() {
        let mut input = Vec::new();
        for i in 0..6 {
            input.push(track(&format!("Track {i}"), "Daft Punk"));
        }
        for i in 0..3 {
            input.push(album(&format!("Album {i}"), "Daft Punk"));
        }
        let out = rank(input, "daft punk");
        assert_eq!(out.len(), 9);
        let head = &out[..5];
        let tracks = head.iter().filter(|i| i.item_type == ItemType::Track).count();
        let albums = head.iter().filter(|i| i.item_type == ItemType::Album).count();
        assert_eq!(tracks, TRACK_SLOTS);
        assert_eq!(albums, ALBUM_SLOTS);
    }

    #[test]
    fn rank_never_drops_a_present_type() {
        let mut input: Vec<SearchSuggestion> =
            (0..10).map(|i| track(&format!("Song {i}"), "Daft Punk")).collect();
        input.push(album("Album", "Daft Punk"));
        let out = rank(input, "daft punk");
        assert!(out.iter().any(|i| i.item_type == ItemType::Album));
        assert_eq!(out.len(), 11);
    }

    #[test]
    fn rank_order_after_slots_follows_score() {
        let input: Vec<SearchSuggestion> = (0..6)
            .map(|i| SearchSuggestion {
                view_count: Some(1_000_000_000 - i as u64 * 100_000_000),
                ..track(&format!("Song {}", (b'A' + i as u8) as char), "Daft Punk")
            })
            .chain(std::iter::once(artist("Daft Punk")))
            .collect();
        let out = rank(input, "daft punk");
        assert_eq!(
            names(&out),
            vec![
                "Daft Punk", "Song A", "Song B", "Song C", "Song D", "Song E", "Song F"
            ]
        );
    }

    #[test]
    fn rank_is_deterministic() {
        let input = vec![
            track("One More Time", "Daft Punk"),
            track("One More Time (Radio Edit)", "Daft Punk"),
            album("Discovery", "Daft Punk"),
            artist("Daft Punk"),
            track("Harder", "Daft Punk"),
            track("Harder", "Daft Punk"),
        ];
        let first = rank(input.clone(), "daft punk");
        let second = rank(input, "daft punk");
        assert_eq!(names(&first), names(&second));
    }

    #[test]
    fn rank_handles_degenerate_items_without_panicking() {
        let input = vec![
            SearchSuggestion {
                view_count: None,
                duration: Some(0),
                ..track("", "")
            },
            SearchSuggestion {
                view_count: Some(u64::MAX),
                duration: Some(u32::MAX),
                ..track("", "")
            },
            artist(""),
            album("   ", ""),
        ];
        let out = rank(input, "");
        assert!(!out.is_empty());
        assert!(out.iter().all(|i| i.item_type != ItemType::Unknown));
    }

    #[test]
    fn rank_empty_query_keeps_items() {
        let out = rank(
            vec![track("A", "X"), track("B", "Y"), artist("C")],
            "",
        );
        assert_eq!(out.len(), 3);
    }

    // ---- rank_flat ----

    #[test]
    fn rank_flat_sorts_by_score_without_layout_and_dedups() {
        let out = rank_flat(
            vec![
                track("Unrelated", "Someone"),
                track("One More Time", "Daft Punk"),
                track("One More Time", "Daft Punk"),
                make(ItemType::Unknown, "x", "y"),
            ],
            "one more time",
        );
        assert_eq!(names(&out), vec!["One More Time", "Unrelated"]);
    }

    #[test]
    fn rank_flat_empty_input_is_empty() {
        assert!(rank_flat(Vec::new(), "q").is_empty());
    }

    #[test]
    fn rank_flat_keeps_the_best_copy_of_a_cross_platform_duplicate() {
        let yt = SearchSuggestion {
            view_count: Some(100_000_000),
            ..track("One More Time", "Daft Punk")
        };
        let bc = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("One More Time", "Daft Punk")
        };
        let out = rank_flat(vec![bc.clone(), yt], "one more time");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].platform, Platform::Youtube));

        // Only the Bandcamp copy: it is kept.
        let only_bc = rank_flat(vec![bc], "one more time");
        assert_eq!(only_bc.len(), 1);
        assert!(matches!(only_bc[0].platform, Platform::Bandcamp));
    }

    #[test]
    fn rank_flat_orders_one_kind_across_platforms_by_score() {
        let out = rank_flat(
            vec![
                artist("Daft Punk Tribute"),
                SearchSuggestion {
                    platform: Platform::Bandcamp,
                    ..artist("Unrelated Name")
                },
                artist("Daft Punk"),
            ],
            "daft punk",
        );
        assert_eq!(names(&out), vec!["Daft Punk", "Daft Punk Tribute", "Unrelated Name"]);
    }

    #[test]
    fn rank_flat_is_deterministic_and_drops_unknown() {
        let input = vec![
            track("One More Time", "Daft Punk"),
            track("One More Time (Radio Edit)", "Daft Punk"),
            make(ItemType::Unknown, "One More Time", "Daft Punk"),
            album("Discovery", "Daft Punk"),
            track("Harder", "Daft Punk"),
        ];
        let first = rank_flat(input.clone(), "daft punk one more time");
        let second = rank_flat(input, "daft punk one more time");
        assert_eq!(names(&first), names(&second));
        assert!(first.iter().all(|i| i.item_type != ItemType::Unknown));
    }

    // ---- regressions from the v1 live test (PLAN.md) ----

    #[test]
    fn regression_roygbiv_official_track_beats_bandcamp_bootleg() {
        let q = "boards of canada roygbiv";
        let official = SearchSuggestion {
            view_count: Some(857_000_000),
            ..track("Roygbiv", "Boards of Canada")
        };
        let bootleg = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Boards of Canada - Roygbiv", "Our New Band")
        };
        // The official track sat at position 8 in the live test, so give it a late native index.
        assert!(score(&official, q, 7, 0.0) > score(&bootleg, q, 0, 0.0));
        let out = rank(vec![bootleg, official], q);
        assert_eq!(out[0].name, "Roygbiv");
        assert_eq!(out[0].band_name, "Boards of Canada");
    }

    #[test]
    fn regression_embedded_artist_title_is_penalised_by_exactly_the_penalty() {
        let q = "boards of canada roygbiv";
        let embedded = track("Boards of Canada - Roygbiv", "Our New Band");
        let plain = track("Boards of Canada Roygbiv", "Our New Band");
        let diff = score(&plain, q, 0, 0.0) - score(&embedded, q, 0, 0.0);
        assert!((diff - EMBEDDED_ARTIST_PENALTY).abs() < 1e-3, "diff {diff}");
    }

    #[test]
    fn regression_embedded_artist_needs_the_artist_in_the_query_and_a_known_band() {
        // "Boards of Canada" is not in the query, so the title is an ordinary title.
        let q = "roygbiv";
        let embedded = track("Boards of Canada - Roygbiv", "Our New Band");
        let plain = track("Boards of Canada Roygbiv", "Our New Band");
        assert_eq!(score(&embedded, q, 0, 0.0), score(&plain, q, 0, 0.0));
        // Placeholder band: nothing to contradict, no penalty.
        let placeholder = track("Boards of Canada - Roygbiv", "Artist");
        let q2 = "boards of canada roygbiv";
        assert_eq!(
            score(&placeholder, q2, 0, 0.0),
            score(&track("Boards of Canada Roygbiv", ""), q2, 0, 0.0)
        );
    }

    #[test]
    fn regression_version_suffix_is_not_an_embedded_artist() {
        let band = words(&normalize("Daft Punk"));
        let q = tokenize("around the world");
        assert!(!embedded_artist_prefix("Around the World - Radio Edit", &band, &q));
        assert!(!embedded_artist_prefix("Around the World - Remastered 2011", &band, &q));
        assert!(!embedded_artist_prefix("Around the World - Single Version", &band, &q));
        assert!(!embedded_artist_prefix("Around the World - Live", &band, &q));
        assert!(!embedded_artist_prefix("Around the World - Live at Wembley", &band, &q));
        // Same title with a version suffix scores exactly like the plain title.
        let plain = track("Around the World", "Daft Punk");
        for name in [
            "Around the World - Radio Edit",
            "Around the World - Remastered 2011",
        ] {
            assert_eq!(
                score(&track(name, "Daft Punk"), "around the world", 0, 0.0),
                score(&plain, "around the world", 0, 0.0),
                "{name}"
            );
        }
    }

    #[test]
    fn regression_real_word_after_dash_still_counts_as_embedded_artist() {
        let band = words(&normalize("Our New Band"));
        let q = tokenize("boards of canada roygbiv");
        assert!(embedded_artist_prefix("Boards of Canada - Roygbiv", &band, &q));
        // Band equal to the left part is an ordinary title, never embedded.
        let daft = words(&normalize("Daft Punk"));
        assert!(!embedded_artist_prefix(
            "Daft Punk - Around the World",
            &daft,
            &tokenize("daft punk around the world")
        ));
    }

    #[test]
    fn regression_daft_punk_artist_field_beats_title_matches() {
        let official = SearchSuggestion {
            view_count: Some(100_000_000),
            ..track("Around the World", "Daft Punk")
        };
        let embedded_bootleg = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Daft Punk - Around the World", "Cover Crew")
        };
        let title_contains = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Harder Daft Punk Forever", "Someone")
        };
        let tribute_title_exact = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Daft Punk", "Daft Punk Tribute Band")
        };
        let out = rank_flat(
            vec![embedded_bootleg, title_contains, tribute_title_exact, official],
            "daft punk",
        );
        assert_eq!(out[0].band_name, "Daft Punk");
        assert_eq!(out[0].name, "Around the World");
        assert_eq!(out.len(), 4);
    }

    #[test]
    fn regression_daft_punk_official_track_beats_bandcamp_bootleg_in_rank() {
        let official = track("Around the World", "Daft Punk");
        let bootleg = SearchSuggestion {
            platform: Platform::Bandcamp,
            ..track("Around the World (Daft Punk Tribute)", "Tribute Act")
        };
        let out = rank(vec![bootleg, official], "daft punk around the world");
        assert_eq!(out[0].band_name, "Daft Punk");
    }

    #[test]
    fn regression_one_more_time_artist_card_is_not_champion_over_the_track() {
        let out = rank(
            vec![artist("One More Time"), track("One More Time", "Daft Punk")],
            "one more time",
        );
        assert_eq!(out[0].item_type, ItemType::Track);
        assert_eq!(out[0].band_name, "Daft Punk");
        // The artist is still in the list, just not first.
        assert!(out.iter().any(|i| i.item_type == ItemType::Artist));
    }

    #[test]
    fn regression_artist_card_keeps_champion_without_a_strong_track() {
        let out = rank(
            vec![artist("One More Time"), track("Harder", "Daft Punk")],
            "one more time",
        );
        assert_eq!(out[0].item_type, ItemType::Artist);
    }

    #[test]
    fn regression_artist_card_needs_an_exact_name_to_be_champion() {
        let out = rank(
            vec![artist("Daft Punk Tribute"), track("Random", "Nobody")],
            "daft punk",
        );
        assert_eq!(out[0].item_type, ItemType::Track);
        assert_eq!(out[0].name, "Random");
    }

    #[test]
    fn regression_artist_champion_check_only_applies_to_real_queries() {
        let out = rank(
            vec![artist("Daft Punk Tribute"), track("Random", "Nobody")],
            "",
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn regression_artist_placeholder_band_is_never_scored() {
        // An artist item that carries "Artist" as band must score the same as one with no band.
        let with_placeholder = make(ItemType::Artist, "Something", "Artist");
        let without = make(ItemType::Artist, "Something", "");
        assert_eq!(
            score(&with_placeholder, "artist", 0, 0.0),
            score(&without, "artist", 0, 0.0)
        );
        // Same for an artist item carrying a real band name: the band is ignored for artists.
        assert_eq!(
            score(&make(ItemType::Artist, "Something", "Daft Punk"), "daft punk", 0, 0.0),
            score(&without, "daft punk", 0, 0.0)
        );
    }

    #[test]
    fn regression_track_with_placeholder_band_does_not_match_on_it() {
        // A track accidentally carrying "Artist" must not gain score from that word.
        let placeholder = track("Some Song", "Artist");
        let empty = track("Some Song", "");
        assert_eq!(
            score(&placeholder, "artist", 0, 0.0),
            score(&empty, "artist", 0, 0.0)
        );
        let real_band = track("Some Song", "Artist Two");
        assert!(score(&real_band, "artist", 0, 0.0) > score(&placeholder, "artist", 0, 0.0));
    }
}
