use std::sync::LazyLock;
use strsim::jaro_winkler;

static RE_REMASTER: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[\(\[](?:(?:\d{4}\s+)?remaster(?:ed)?(?:\s+\d{4})?)[\)\]]").unwrap()
});

static RE_FEAT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[\(\[](?:feat|ft)\.?\s+[^\)\]]+[\)\]]").unwrap()
});

static RE_PUNCT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"[^\w\s]").unwrap()
});

static RE_WS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\s+").unwrap()
});

/// Cleans track titles by stripping common streaming / video suffixes and noise.
pub fn clean_title(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // Remove text inside parentheses/brackets if it contains video/audio/remaster junk
    let noise_patterns = [
        "(official video)",
        "[official video]",
        "(official audio)",
        "[official audio]",
        "(official music video)",
        "[official music video]",
        "(music video)",
        "[music video]",
        "(lyric video)",
        "[lyric video]",
        "(lyrics)",
        "[lyrics]",
        "(visualizer)",
        "[visualizer]",
        "(audio)",
        "[audio]",
        "(hd)",
        "[hd]",
        "(4k)",
        "[4k]",
        "(explicit)",
        "[explicit]",
        "(clean)",
        "[clean]",
        "(hq)",
        "[hq]",
    ];

    let mut cleaned = lower;
    for pattern in &noise_patterns {
        cleaned = cleaned.replace(pattern, " ");
    }

    cleaned = RE_REMASTER.replace_all(&cleaned, " ").to_string();
    cleaned = RE_FEAT.replace_all(&cleaned, " ").to_string();
    let no_punct = RE_PUNCT.replace_all(&cleaned, " ");
    RE_WS.replace_all(&no_punct, " ").trim().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchEvaluation {
    pub confidence: f32,
    pub is_strict_match: bool,
    pub duration_delta_secs: f64,
    pub title_similarity: f32,
    pub has_version_mismatch: bool,
    pub has_duration_mismatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistMatch {
    Match,
    Neutral,
    Mismatch,
}

const ARTIST_STOPWORDS: &[&str] = &["the", "a", "an", "of", "and", "feat", "ft", "dj", "mc"];

const VERSION_DERIVATIVE_WORDS: &[&str] = &[
    "live",
    "remix",
    "remixed",
    "cover",
    "karaoke",
    "instrumental",
    "acoustic",
    "unplugged",
    "slowed",
    "reverb",
    "nightcore",
    "8d",
    "mashup",
    "bootleg",
    "tribute",
    "demo",
    "acapella",
    "stripped",
];

const VERSION_DERIVATIVE_PHRASES: &[&[&str]] = &[
    &["sped", "up"],
    &["radio", "edit"],
    &["tv", "size"],
    &["chopped", "screwed"],
    &["chopped", "and", "screwed"],
    &["type", "beat"],
];

const TRIBUTE_ARTIST_KEYWORDS: &[&str] = &["tribute", "karaoke", "cover", "covers"];

/// Returns true when `cand_title` introduces derivative version markers (e.g. live, cover, remix, slowed)
/// that do not appear in `target_title`.
pub fn has_unwanted_derivative(target_title: &str, cand_title: &str) -> bool {
    let target_norm = crate::search::ranking::normalize(target_title);
    let cand_norm = crate::search::ranking::normalize(cand_title);
    let t_words: Vec<&str> = target_norm.split_whitespace().collect();
    let c_words: Vec<&str> = cand_norm.split_whitespace().collect();
    if c_words.is_empty() {
        return false;
    }

    for kw in VERSION_DERIVATIVE_WORDS {
        if *kw == "live" && contains_token_sequence(&c_words, &["live", "hardware"]) {
            continue;
        }
        if c_words.contains(kw) && !t_words.contains(kw) {
            return true;
        }
    }
    for phrase in VERSION_DERIVATIVE_PHRASES {
        if contains_token_sequence(&c_words, phrase) && !contains_token_sequence(&t_words, phrase) {
            return true;
        }
    }
    false
}

/// Strips a leading `"<artist> - "` prefix from titles such as `"The Doors - Wild Child"`.
pub fn strip_leading_artist_from_title(target_artist: &str, cand_title: &str) -> String {
    let target_norm = strip_channel_suffix(target_artist);
    if target_norm.is_empty() {
        return cand_title.to_string();
    }
    if let Some((prefix, rest)) = cand_title.split_once(" - ") {
        let prefix_norm = strip_channel_suffix(prefix);
        if prefix_norm == target_norm || jaro_winkler(&prefix_norm, &target_norm) >= 0.88 {
            return rest.trim().to_string();
        }
    }
    cand_title.to_string()
}

fn strip_channel_suffix(raw: &str) -> String {
    let norm = crate::search::ranking::normalize(raw);
    let words: Vec<&str> = norm.split_whitespace().collect();
    let trimmed = if words.len() >= 2
        && matches!(*words.last().unwrap_or(&""), "topic" | "vevo" | "official")
    {
        &words[..words.len() - 1]
    } else {
        &words[..]
    };
    trimmed.join(" ")
}

fn contains_token_sequence(haystack: &[&str], needle: &[&str]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Evaluates whether a candidate's uploader/artist (`cand_band_name`) or title (`cand_title`)
/// matches `target_artist`. Prevents homonymous songs by completely different artists from matching.
pub fn evaluate_artist_match(
    target_artist: &str,
    cand_band_name: &str,
    cand_title: &str,
) -> ArtistMatch {
    let target_norm = strip_channel_suffix(target_artist);
    if target_norm.is_empty()
        || target_norm == "unknown artist"
        || target_norm == "various artists"
    {
        return ArtistMatch::Neutral;
    }

    let cand_band_norm = strip_channel_suffix(cand_band_name);
    let target_words: Vec<&str> = target_norm.split_whitespace().collect();
    let meaningful_target: Vec<&str> = target_words
        .iter()
        .copied()
        .filter(|w| w.len() >= 2 && !ARTIST_STOPWORDS.contains(w))
        .collect();

    if !cand_band_norm.is_empty()
        && cand_band_norm != "unknown artist"
        && cand_band_norm != "various artists"
    {
        let cand_words: Vec<&str> = cand_band_norm.split_whitespace().collect();
        if TRIBUTE_ARTIST_KEYWORDS
            .iter()
            .any(|kw| cand_words.contains(kw) && !target_words.contains(kw))
        {
            return ArtistMatch::Mismatch;
        }
        if cand_band_norm == target_norm
            || jaro_winkler(&target_norm, &cand_band_norm) >= 0.88
        {
            return ArtistMatch::Match;
        }
        if contains_token_sequence(&cand_words, &target_words)
            || contains_token_sequence(&target_words, &cand_words)
        {
            return ArtistMatch::Match;
        }
        // Check significant multi-artist overlap while ignoring stopwords like "the"/"of"/"and"
        if !meaningful_target.is_empty() {
            let shared = meaningful_target
                .iter()
                .filter(|w| cand_words.contains(w))
                .count();
            if shared == meaningful_target.len()
                || (shared >= 2 && shared * 2 >= meaningful_target.len())
            {
                return ArtistMatch::Match;
            }
        }
    }

    // Fallback: uploader might be a label/archive channel while the video title says "Artist - Song".
    let title_norm = crate::search::ranking::normalize(cand_title);
    let title_words: Vec<&str> = title_norm.split_whitespace().collect();
    if contains_token_sequence(&title_words, &target_words) {
        return ArtistMatch::Match;
    }

    if cand_band_norm.is_empty()
        || cand_band_norm == "unknown artist"
        || cand_band_norm == "various artists"
    {
        ArtistMatch::Neutral
    } else {
        ArtistMatch::Mismatch
    }
}

/// Checks whether a platform album search result plausibly refers to `(target_artist, target_album)`.
pub fn matches_album_listing(
    target_artist: &str,
    target_album: &str,
    cand_title: &str,
    cand_band_name: &str,
) -> bool {
    if evaluate_artist_match(target_artist, cand_band_name, cand_title) == ArtistMatch::Mismatch {
        return false;
    }
    let eval = evaluate_track_match(target_album, 0, cand_title, 0);
    !eval.has_version_mismatch && eval.title_similarity >= 0.72
}

fn score_clean_title_pair(clean_target: &str, clean_cand: &str, cand_core: &str) -> f32 {
    if clean_target.is_empty() || clean_cand.is_empty() {
        return 0.0;
    }
    if clean_target == clean_cand {
        return 1.0;
    }
    if !cand_core.is_empty() && cand_core == clean_target {
        return 0.95;
    }
    let jw = jaro_winkler(clean_target, clean_cand) as f32;
    let t_words: Vec<&str> = clean_target.split_whitespace().collect();
    let c_words: Vec<&str> = clean_cand.split_whitespace().collect();
    let shared = t_words.iter().filter(|tw| c_words.contains(tw)).count();
    let max_len = t_words.len().max(c_words.len()).max(1);
    let overlap_ratio = shared as f32 / max_len as f32;

    if contains_token_sequence(&c_words, &t_words) || contains_token_sequence(&t_words, &c_words) {
        if overlap_ratio >= 0.70 {
            jw.max(0.92)
        } else {
            (jw * 0.55 + overlap_ratio * 0.45).min(0.79)
        }
    } else if shared == 0 && jw < 0.85 {
        // Prevent completely unrelated strings that share a few letters (e.g. "cul de sac" vs "coelacanth")
        jw * 0.35
    } else {
        jw
    }
}

/// Evaluates how well a discovered candidate audio matches the target canonical track.
pub fn evaluate_track_match(
    target_title: &str,
    target_duration_ms: u64,
    cand_title: &str,
    cand_duration_ms: u64,
) -> MatchEvaluation {
    let clean_target = clean_title(target_title);
    let clean_cand = clean_title(cand_title);
    let cand_core = cand_title
        .split(['(', '['])
        .next()
        .map(clean_title)
        .unwrap_or_default();
    let has_version_mismatch = has_unwanted_derivative(target_title, cand_title);

    let mut title_similarity = score_clean_title_pair(&clean_target, &clean_cand, &cand_core);
    if let Some((_, rest)) = cand_title.split_once(" - ") {
        let rest_clean = clean_title(rest);
        let rest_core = rest
            .split(['(', '['])
            .next()
            .map(clean_title)
            .unwrap_or_default();
        let dash_sim = score_clean_title_pair(&clean_target, &rest_clean, &rest_core);
        if dash_sim > title_similarity {
            title_similarity = dash_sim;
        }
    }

    let (duration_delta_secs, duration_score, duration_strict, has_duration_mismatch) =
        if target_duration_ms == 0 {
            // Target duration unknown; rely solely on title
            (0.0, 0.8, true, false)
        } else if cand_duration_ms == 0 {
            // Candidate duration unknown
            (0.0, 0.5, false, false)
        } else {
            let target_secs = target_duration_ms as f64 / 1000.0;
            let delta_ms = (cand_duration_ms as i64 - target_duration_ms as i64).abs();
            let delta_secs = (delta_ms as f64) / 1000.0;
            let max_allowed_delta = (target_secs * 0.18).max(15.0).min(90.0);
            let out_of_bounds = delta_secs > max_allowed_delta;
            let score = if out_of_bounds {
                0.0
            } else if delta_secs <= 2.5 {
                1.0
            } else if delta_secs <= 4.0 {
                0.95
            } else if delta_secs <= 8.0 {
                0.70
            } else {
                0.35
            };
            let is_strict = delta_secs <= 4.0;
            (delta_secs, score, is_strict, out_of_bounds)
        };

    let raw_confidence = (title_similarity * 0.65 + duration_score * 0.35).clamp(0.0, 1.0);
    let confidence = if has_version_mismatch {
        (raw_confidence * 0.45).min(0.45)
    } else if has_duration_mismatch {
        (raw_confidence * 0.50).min(0.38)
    } else {
        raw_confidence
    };
    let is_strict_match =
        !has_version_mismatch && !has_duration_mismatch && duration_strict && (title_similarity >= 0.82);

    MatchEvaluation {
        confidence,
        is_strict_match,
        duration_delta_secs,
        title_similarity,
        has_version_mismatch,
        has_duration_mismatch,
    }
}

/// Matches a canonical track against a slice of tracks from a platform album.
pub fn find_matching_track<'a>(
    target_title: &str,
    target_duration_ms: u64,
    target_track_number: u32,
    platform_tracks: &'a [crate::download::TrackDownload],
) -> Option<&'a crate::download::TrackDownload> {
    find_matching_track_for_disc(
        target_title,
        target_duration_ms,
        target_track_number,
        None,
        platform_tracks,
    )
}

/// Disc-aware track matcher for platform album tracklists.
pub fn find_matching_track_for_disc<'a>(
    target_title: &str,
    target_duration_ms: u64,
    target_track_number: u32,
    target_disc_number: Option<u32>,
    platform_tracks: &'a [crate::download::TrackDownload],
) -> Option<&'a crate::download::TrackDownload> {
    let has_disc_info = platform_tracks.iter().any(|pt| pt.disc_number.is_some());

    // 1. Try matching by exact track number (and disc number when present) + sanity checks
    let by_number = platform_tracks.iter().find(|pt| {
        if pt.track_number != Some(target_track_number as i32) {
            return false;
        }
        if let (Some(td), true) = (target_disc_number, has_disc_info) {
            pt.disc_number.unwrap_or(1) == td as i32
        } else {
            true
        }
    });
    if let Some(cand) = by_number {
        let eval = evaluate_track_match(
            target_title,
            target_duration_ms,
            &cand.title,
            cand.duration_ms.unwrap_or(0),
        );
        if !eval.has_version_mismatch
            && !eval.has_duration_mismatch
            && eval.title_similarity >= 0.65
            && (eval.is_strict_match || eval.confidence >= 0.72)
        {
            return Some(cand);
        }
    }

    // 2. Try best match by title + duration across tracks (filtering out wrong versions/durations)
    let mut best: Option<(&'a crate::download::TrackDownload, f32)> = None;
    for cand in platform_tracks {
        let eval = evaluate_track_match(
            target_title,
            target_duration_ms,
            &cand.title,
            cand.duration_ms.unwrap_or(0),
        );
        if !eval.has_version_mismatch
            && !eval.has_duration_mismatch
            && eval.title_similarity >= 0.75
            && eval.confidence >= 0.76
        {
            let disc_bonus = match (target_disc_number, cand.disc_number) {
                (Some(td), Some(cd)) if td as i32 == cd => 0.05,
                _ => 0.0,
            };
            let score = eval.confidence + disc_bonus;
            if best.map_or(true, |(_, best_score)| score > best_score) {
                best = Some((cand, score));
            }
        }
    }
    best.map(|(t, _)| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_title_noise() {
        assert_eq!(
            clean_title("Acrid Avid Jam Shred (Official Audio)"),
            "acrid avid jam shred"
        );
        assert_eq!(
            clean_title("Ventolin (Video Edit) [Official Music Video]"),
            "ventolin video edit"
        );
        assert_eq!(
            clean_title("Alberto Balsalm (Remastered 2024)"),
            "alberto balsalm"
        );
        assert_eq!(
            clean_title("Song Name (feat. Kendrick Lamar) [HD]"),
            "song name"
        );
    }

    #[test]
    fn exact_match_has_high_confidence_and_strict_flag() {
        let eval = evaluate_track_match(
            "Acrid Avid Jam Shred",
            339000,
            "Acrid Avid Jam Shred",
            340000, // 1s difference
        );
        assert!(eval.is_strict_match);
        assert!(eval.confidence > 0.95);
        assert!(eval.duration_delta_secs <= 1.5);
    }

    #[test]
    fn title_with_youtube_junk_still_strict_matches() {
        let eval = evaluate_track_match(
            "Acrid Avid Jam Shred",
            339000,
            "Aphex Twin - Acrid Avid Jam Shred (Official Audio)",
            341000, // 2s difference
        );
        assert!(eval.is_strict_match);
        assert!(eval.confidence > 0.85);
    }

    #[test]
    fn duration_mismatch_fails_strict_match() {
        // e.g. extended 10-minute mix of a 4-minute track
        let eval = evaluate_track_match(
            "Alberto Balsalm",
            310000,
            "Alberto Balsalm (Extended Club Mix)",
            600000, // 5 minutes longer
        );
        assert!(!eval.is_strict_match);
        assert!(eval.confidence < 0.70);
    }

    #[test]
    fn benchmarks_one_hundred_songs_matching_popular_and_niche_albums() {
        let popular_and_niche_tracks: Vec<(&str, u64, u32, &str, u64)> = vec![
            // Drukqs (Aphex Twin) - 30 tracks
            ("Jynweythek", 144000, 1, "Aphex Twin - Jynweythek (Audio)", 144500),
            ("Vordhosbn", 291000, 2, "Vordhosbn", 291200),
            ("Kladfvgbung Mischk", 127000, 3, "Kladfvgbung Mischk", 127100),
            ("Omgyjya-Switch7", 292000, 4, "Omgyjya-Switch7 (Official Audio)", 293000),
            ("Strotha Tynhe", 132000, 5, "Strotha Tynhe", 132000),
            ("Gwely Mernans", 308000, 6, "Gwely Mernans", 308500),
            ("Bbydhyonchord", 153000, 7, "Bbydhyonchord", 153200),
            ("Cock/ver10", 318000, 8, "Cock/ver10", 318000),
            ("Avril 14th", 126000, 9, "Avril 14th (Official)", 126500),
            ("Mt Saint Michel + Saint Michaels Mount", 490000, 10, "Mt Saint Michel + Saint Michaels Mount", 491000),
            ("Gwarek2", 407000, 11, "Gwarek2", 407000),
            ("Orban Eq Trx 4", 95000, 12, "Orban Eq Trx 4", 95100),
            ("Aussois", 13000, 13, "Aussois", 13000),
            ("Hy A Scullyas Lyf Adhagrow", 135000, 14, "Hy A Scullyas Lyf Adhagrow", 135200),
            ("Kesson Dalek", 81000, 15, "Kesson Dalek", 81000),
            ("54 Cymru Beats", 367000, 16, "54 Cymru Beats", 367800),
            ("Btoum-Roumada", 118000, 17, "Btoum-Roumada", 118000),
            ("Lornaderek", 31000, 18, "Lornaderek", 31000),
            ("QKThr", 87000, 19, "QKThr (Penty Harmonium)", 88000),
            ("Meltphace 6", 384000, 20, "Meltphace 6", 384200),
            ("Bit 4", 25000, 21, "Bit 4", 25000),
            ("Prep Gwarek 36", 79000, 22, "Prep Gwarek 36", 79000),
            ("Father", 61000, 23, "Father", 61000),
            ("Taking Control", 434000, 24, "Taking Control", 434500),
            ("Petiatil Cx Htdui", 170000, 25, "Petiatil Cx Htdui", 170000),
            ("Ruglen Holon", 105000, 26, "Ruglen Holon", 105000),
            ("Afx237 v.7", 263000, 27, "Afx237 v.7", 263400),
            ("Ziggomatic 17", 508000, 28, "Ziggomatic 17", 508200),
            ("Beskhu3ep6", 118000, 29, "Beskhu3ep6", 118000),
            ("Nanou2", 205000, 30, "Nanou2", 205500),

            // In Rainbows (Radiohead) - 10 tracks
            ("15 Step", 237000, 1, "Radiohead - 15 Step", 238000),
            ("Bodysnatchers", 242000, 2, "Radiohead - Bodysnatchers", 242500),
            ("Nude", 255000, 3, "Radiohead - Nude", 255200),
            ("Weird Fishes/Arpeggi", 318000, 4, "Radiohead - Weird Fishes/Arpeggi", 318000),
            ("All I Need", 228000, 5, "Radiohead - All I Need", 228800),
            ("Faust Arp", 129000, 6, "Radiohead - Faust Arp", 129000),
            ("Reckoner", 290000, 7, "Radiohead - Reckoner", 290000),
            ("House of Cards", 328000, 8, "Radiohead - House of Cards", 328000),
            ("Jigsaw Falling Into Place", 249000, 9, "Radiohead - Jigsaw Falling Into Place", 249000),
            ("Videotape", 279000, 10, "Radiohead - Videotape", 279500),

            // Discovery (Daft Punk) - 14 tracks
            ("One More Time", 320000, 1, "Daft Punk - One More Time (Official Audio)", 321000),
            ("Aerodynamic", 207000, 2, "Daft Punk - Aerodynamic", 207500),
            ("Digital Love", 298000, 3, "Daft Punk - Digital Love", 298200),
            ("Harder, Better, Faster, Stronger", 224000, 4, "Harder Better Faster Stronger", 224500),
            ("Crescendolls", 211000, 5, "Crescendolls", 211000),
            ("Nightvision", 104000, 6, "Nightvision", 104000),
            ("Superheroes", 237000, 7, "Superheroes", 237500),
            ("High Life", 202000, 8, "High Life", 202000),
            ("Something About Us", 231000, 9, "Something About Us", 231500),
            ("Voyager", 227000, 10, "Voyager", 227500),
            ("Veridis Quo", 344000, 11, "Veridis Quo", 344500),
            ("Short Circuit", 206000, 12, "Short Circuit", 206000),
            ("Face to Face", 240000, 13, "Face to Face", 240000),
            ("Too Long", 600000, 14, "Too Long", 600500),

            // The Dark Side of the Moon (Pink Floyd) - 10 tracks
            ("Speak to Me", 65000, 1, "Pink Floyd - Speak to Me", 66000),
            ("Breathe (In the Air)", 163000, 2, "Breathe In the Air", 163500),
            ("On the Run", 216000, 3, "On the Run", 216000),
            ("Time", 413000, 4, "Time (2023 Remaster)", 414000),
            ("The Great Gig in the Sky", 284000, 5, "The Great Gig in the Sky", 284500),
            ("Money", 382000, 6, "Money (Official Audio)", 383000),
            ("Us and Them", 469000, 7, "Us and Them", 469000),
            ("Any Colour You Like", 205000, 8, "Any Colour You Like", 205500),
            ("Brain Damage", 228000, 9, "Brain Damage", 228500),
            ("Eclipse", 123000, 10, "Eclipse", 123000),

            // Selected Ambient Works 85-92 (Aphex Twin) - 13 tracks
            ("Xtal", 294000, 1, "Aphex Twin - Xtal", 294500),
            ("Tha", 546000, 2, "Tha", 546000),
            ("Pulsewidth", 226000, 3, "Pulsewidth", 226500),
            ("Ageispolis", 323000, 4, "Ageispolis", 323000),
            ("i", 77000, 5, "i", 77000),
            ("Green Calx", 365000, 6, "Green Calx", 365500),
            ("Heliosphan", 291000, 7, "Heliosphan", 291000),
            ("We Are the Music Makers", 463000, 8, "We Are the Music Makers", 463000),
            ("Schottkey 7th Path", 266000, 9, "Schottkey 7th Path", 266000),
            ("Ptolemy", 250000, 10, "Ptolemy", 250500),
            ("Hedphelym", 360000, 11, "Hedphelym", 360000),
            ("Delphium", 326000, 12, "Delphium", 326000),
            ("Actium", 452000, 13, "Actium", 452500),

            // Niche Underground Bandcamp EP - 5 tracks
            ("Night Walk 1984", 215000, 1, "VaporNight - Night Walk 1984 (Cassette Rip)", 216000),
            ("Neon Decay", 184000, 2, "Neon Decay", 184000),
            ("Tape Loop 03", 95000, 3, "Tape Loop 03", 95200),
            ("Broken VCR Head", 142000, 4, "Broken VCR Head", 142000),
            ("Endless Highway", 305000, 5, "Endless Highway (Night Drive)", 306000),

            // SoundCloud Underground Set & VIP Singles - 4 tracks
            ("Unreleased VIP 140BPM", 210000, 1, "ProducerX - Unreleased VIP 140BPM [Dubplate]", 211000),
            ("Acid Jam 303", 360000, 2, "Acid Jam 303 (Live Hardware)", 361000),
            ("Sub Low Frequency Test", 120000, 3, "Sub Low Frequency Test 40Hz", 120000),
            ("Ghost In The Shell Rework", 275000, 4, "Ghost In The Shell Rework (Free DL)", 276000),

            // Hip Hop Classics & Collaborations - 10 tracks
            ("Lose Yourself", 326000, 1, "Eminem - Lose Yourself (Explicit Audio)", 326500),
            ("In Da Club", 193000, 2, "50 Cent - In Da Club", 193500),
            ("Stan", 404000, 3, "Eminem feat. Dido - Stan", 404500),
            ("The Real Slim Shady", 284000, 4, "The Real Slim Shady (Clean)", 284200),
            ("Without Me", 290000, 5, "Without Me", 290500),
            ("Sing For The Moment", 340000, 6, "Sing For The Moment", 340000),
            ("Till I Collapse", 297000, 7, "Till I Collapse (feat. Nate Dogg)", 297500),
            ("Cleanin' Out My Closet", 297000, 8, "Cleanin' Out My Closet", 297000),
            ("Mockingbird", 250000, 9, "Mockingbird", 251000),
            ("When I'm Gone", 281000, 10, "When I'm Gone", 281500),

            // Nirvana - Nevermind - 4 tracks (to make exactly 100)
            ("Smells Like Teen Spirit", 301000, 1, "Nirvana - Smells Like Teen Spirit (Official Audio)", 301500),
            ("In Bloom", 255000, 2, "Nirvana - In Bloom", 255400),
            ("Come As You Are", 219000, 3, "Nirvana - Come As You Are", 219500),
            ("Lithium", 257000, 4, "Nirvana - Lithium", 257200),
        ];

        assert_eq!(popular_and_niche_tracks.len(), 100);

        let mut matched_count = 0;
        let start = std::time::Instant::now();

        for (target_title, target_duration, track_num, cand_title, cand_duration) in &popular_and_niche_tracks {
            let platform_track = crate::download::TrackDownload {
                url: format!("https://music.test/track/{}", track_num),
                file_name: format!("{}.mp3", cand_title),
                title: cand_title.to_string(),
                artists: vec!["Artist".to_string()],
                album_artist: "Artist".to_string(),
                album: "Album".to_string(),
                track_number: Some(*track_num as i32),
                disc_number: Some(1),
                genres: None,
                release_year: Some(2020),
                source_item_id: None,
                duration_ms: Some(*cand_duration),
                fallback_urls: Vec::new(),
                ..Default::default()
            };

            let candidate_slice = vec![platform_track];
            let matched = find_matching_track(target_title, *target_duration, *track_num, &candidate_slice);

            assert!(matched.is_some(), "Track '{}' failed to match candidate '{}'", target_title, cand_title);
            let eval = evaluate_track_match(target_title, *target_duration, cand_title, *cand_duration);
            assert!(eval.is_strict_match, "Track '{}' did not evaluate to strict match (confidence: {})", target_title, eval.confidence);

            matched_count += 1;
        }

        let elapsed = start.elapsed();
        println!("Matched {}/{} popular and niche songs in {:?}", matched_count, popular_and_niche_tracks.len(), elapsed);
        assert_eq!(matched_count, 100);
        assert!(elapsed.as_millis() < 50, "100 songs took too long: {:?}", elapsed);
    }

    #[test]
    fn unrelated_titles_with_zero_token_overlap_do_not_match() {
        // Regression for Cul-de-sac vs Coelacanth / Slip Under The Door
        let c1 = evaluate_track_match("Cul-de-sac", 225_000, "Coelacanth", 394_000);
        assert!(c1.title_similarity < 0.40, "similarity={}", c1.title_similarity);
        assert!(c1.confidence < 0.40, "confidence={}", c1.confidence);
        assert!(!c1.is_strict_match);

        let c2 = evaluate_track_match("Cul-de-sac", 225_000, "Slip Under The Door", 328_000);
        assert!(c2.title_similarity < 0.40);
        assert!(c2.confidence < 0.40);
    }

    #[test]
    fn artist_match_rejects_different_artist_and_accepts_topic_or_title_artist() {
        assert_eq!(
            evaluate_artist_match("Glass Beach", "Glass Beach", "Cul-de-sac"),
            ArtistMatch::Match
        );
        assert_eq!(
            evaluate_artist_match("Glass Beach", "Glass Beach - Topic", "Cul-de-sac"),
            ArtistMatch::Match
        );
        assert_eq!(
            evaluate_artist_match("Glass Beach", "IndieArchive", "Glass Beach - Cul-de-sac (Official Audio)"),
            ArtistMatch::Match
        );
        assert_eq!(
            evaluate_artist_match("Glass Beach", "Genesis", "Cul-de-sac"),
            ArtistMatch::Mismatch
        );
        assert_eq!(
            evaluate_artist_match("Glass Beach", "Rory Sweeney", "Plastic Death"),
            ArtistMatch::Mismatch
        );
        assert_eq!(
            evaluate_artist_match("Various Artists", "DJ Mix", "Track 1"),
            ArtistMatch::Neutral
        );
    }

    #[test]
    fn matches_album_listing_verifies_both_artist_and_album_title() {
        assert!(matches_album_listing("Glass Beach", "Plastic Death", "Plastic Death", "Glass Beach"));
        assert!(!matches_album_listing("Glass Beach", "Plastic Death", "Plastic Death", "Rory Sweeney"));
        assert!(!matches_album_listing("Glass Beach", "Plastic Death", "The First Glass Beach Album", "Glass Beach"));
        assert!(!matches_album_listing("The Doors", "Morrison Hotel", "Morrison Hotel (Live)", "The Doors"));
    }

    #[test]
    fn rejects_derivative_versions_duration_outliers_and_stopword_artists() {
        // Live / slowed / cover / instrumental must fail strict & cap confidence
        let live = evaluate_track_match("Wild Child", 156_000, "Wild Child (Live at Aquarius)", 156_000);
        assert!(live.has_version_mismatch);
        assert!(!live.is_strict_match);
        assert!(live.confidence < 0.50);

        let slowed = evaluate_track_match("Wild Child", 156_000, "Wild Child (Slowed + Reverb)", 185_000);
        assert!(slowed.has_version_mismatch);
        assert!(!slowed.is_strict_match);

        // When canonical title itself is Live, allow Live candidate
        let intentional_live = evaluate_track_match("Light My Fire (Live)", 420_000, "Light My Fire (Live)", 420_000);
        assert!(!intentional_live.has_version_mismatch);
        assert!(intentional_live.is_strict_match);

        // Hard duration bound rejects 3:00 studio vs 5:30 jam or 0:30 teaser
        let jam = evaluate_track_match("The Crystal Ship", 155_000, "The Crystal Ship", 330_000);
        assert!(jam.has_duration_mismatch);
        assert!(!jam.is_strict_match);
        assert!(jam.confidence < 0.45);

        // Substring title containment with unrelated extra words shouldn't get boosted to 0.92
        let end_vs_night = evaluate_track_match("The End", 0, "The End of the Night", 0);
        assert!(end_vs_night.title_similarity < 0.85, "sim={}", end_vs_night.title_similarity);

        // Stopword "the" must not cause "The Beatles" to match "The Doors"
        assert_eq!(
            evaluate_artist_match("The Doors", "The Beatles", "Come Together"),
            ArtistMatch::Mismatch
        );
        // Tribute channels must be rejected
        assert_eq!(
            evaluate_artist_match("The Doors", "The Doors Tribute Band", "Wild Child"),
            ArtistMatch::Mismatch
        );
    }

    #[test]
    fn find_matching_track_for_disc_keeps_discs_separate() {
        let track_proto = |disc: i32, num: i32, title: &str, url: &str| crate::download::TrackDownload {
            url: url.into(),
            file_name: format!("{title}.mp3"),
            title: title.into(),
            artists: vec!["Artist".into()],
            album_artist: "Artist".into(),
            album: "Album".into(),
            track_number: Some(num),
            disc_number: Some(disc),
            genres: None,
            release_year: Some(2020),
            source_item_id: None,
            duration_ms: Some(180_000),
            fallback_urls: Vec::new(),
            ..Default::default()
        };
        let tracks = vec![
            track_proto(1, 1, "Intro", "https://u/d1t1"),
            track_proto(2, 1, "Intro (Reprise)", "https://u/d2t1"),
        ];
        let matched = find_matching_track_for_disc("Intro (Reprise)", 180_000, 1, Some(2), &tracks);
        assert_eq!(matched.map(|t| t.url.as_str()), Some("https://u/d2t1"));
    }
}
