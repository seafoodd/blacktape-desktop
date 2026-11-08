//! LRCLIB provider (https://lrclib.net) — free, supports synced (LRC) lyrics.

use super::common::{
    build_url, get_json, is_acceptable, track_matches, LyricsSource, APP_USER_AGENT,
    INSTRUMENTAL_LYRICS,
};

const SOURCE: &str = "https://lrclib.net";
/// Max duration difference (seconds) for a search hit to count as the same recording.
const DURATION_TOLERANCE_SECS: f64 = 5.0;

#[derive(serde::Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct LrclibTrack {
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    pub instrumental: bool,
    pub duration: Option<f64>,
    pub track_name: Option<String>,
    pub artist_name: Option<String>,
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| is_acceptable(v))
}

/// Best lyrics of a single track: synced > plain > instrumental marker.
pub fn pick_from_track(t: &LrclibTrack) -> Option<String> {
    if let Some(s) = non_empty(&t.synced_lyrics) {
        return Some(s.to_string());
    }
    if let Some(p) = non_empty(&t.plain_lyrics) {
        return Some(p.to_string());
    }
    t.instrumental.then(|| INSTRUMENTAL_LYRICS.to_string())
}

fn duration_matches(t: &LrclibTrack, wanted_secs: Option<u64>) -> bool {
    match (t.duration, wanted_secs) {
        (Some(d), Some(w)) if w > 0 => (d - w as f64).abs() <= DURATION_TOLERANCE_SECS,
        _ => true, // unknown on either side: don't discriminate
    }
}

/// Picks from search results. Order: synced (right duration) > synced (any)
/// > plain (right duration) > plain (any) > instrumental.
pub fn pick_from_search(results: &[LrclibTrack], wanted_secs: Option<u64>) -> Option<String> {
    let synced = |t: &&LrclibTrack| non_empty(&t.synced_lyrics).is_some();
    let plain = |t: &&LrclibTrack| non_empty(&t.plain_lyrics).is_some();

    let found = results
        .iter()
        .filter(synced)
        .find(|t| duration_matches(t, wanted_secs))
        .or_else(|| results.iter().find(synced))
        .map(|t| t.synced_lyrics.clone())
        .or_else(|| {
            results
                .iter()
                .filter(plain)
                .find(|t| duration_matches(t, wanted_secs))
                .or_else(|| results.iter().find(plain))
                .map(|t| t.plain_lyrics.clone())
        })
        .flatten();

    found.or_else(|| {
        results
            .iter()
            .any(|t| t.instrumental)
            .then(|| INSTRUMENTAL_LYRICS.to_string())
    })
}

/// Keeps only search hits that are really the requested song.
pub fn filter_matching(results: Vec<LrclibTrack>, artist: &str, title: &str) -> Vec<LrclibTrack> {
    results
        .into_iter()
        .filter(|t| {
            track_matches(
                artist,
                title,
                t.artist_name.as_deref().unwrap_or(""),
                t.track_name.as_deref().unwrap_or(""),
            )
        })
        .collect()
}

pub async fn fetch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: Option<u64>,
) -> Result<LyricsSource, String> {
    let ok = |lyrics: String| LyricsSource {
        lyrics,
        source: SOURCE.to_string(),
    };

    // 1. Direct match: /api/get
    let duration_str = duration_secs.filter(|d| *d > 0).map(|d| d.to_string());
    let mut params = vec![("artist_name", artist), ("track_name", title)];
    if let Some(d) = duration_str.as_deref() {
        params.push(("duration", d));
    }
    if let Some(alb) = album.filter(|a| !a.trim().is_empty() && *a != "Unknown Album") {
        params.push(("album_name", alb));
    }
    let get_url = build_url("https://lrclib.net/api/get", &params);
    if let Ok(track) = get_json::<LrclibTrack>(client, &get_url, APP_USER_AGENT).await {
        if let Some(l) = pick_from_track(&track) {
            return Ok(ok(l));
        }
    }

    // 2. Search fallback: /api/search
    let search_url = build_url(
        "https://lrclib.net/api/search",
        &[("artist_name", artist), ("track_name", title)],
    );
    if let Ok(results) = get_json::<Vec<LrclibTrack>>(client, &search_url, APP_USER_AGENT).await {
        let results = filter_matching(results, artist, title);
        if let Some(l) = pick_from_search(&results, duration_secs) {
            return Ok(ok(l));
        }
    }

    Err("LRCLIB: no lyrics found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED: &str = "[00:01.00] synced line";

    fn track(
        synced: Option<&str>,
        plain: Option<&str>,
        instrumental: bool,
        duration: Option<f64>,
    ) -> LrclibTrack {
        LrclibTrack {
            synced_lyrics: synced.map(str::to_string),
            plain_lyrics: plain.map(str::to_string),
            instrumental,
            duration,
            ..Default::default()
        }
    }

    #[test]
    fn pick_from_track_prefers_synced_over_plain() {
        let t = track(Some(SYNCED), Some("plain"), false, None);
        assert_eq!(pick_from_track(&t).as_deref(), Some(SYNCED));
    }

    #[test]
    fn pick_from_track_uses_plain_when_synced_missing_or_blank() {
        let none = track(None, Some("plain"), false, None);
        assert_eq!(pick_from_track(&none).as_deref(), Some("plain"));

        let blank = track(Some("   "), Some("plain"), false, None);
        assert_eq!(pick_from_track(&blank).as_deref(), Some("plain"));
    }

    #[test]
    fn pick_from_track_instrumental_flag_gives_marker() {
        let t = track(None, None, true, None);
        assert_eq!(pick_from_track(&t).as_deref(), Some(INSTRUMENTAL_LYRICS));
    }

    #[test]
    fn pick_from_track_nothing_gives_none() {
        let t = track(None, None, false, None);
        assert_eq!(pick_from_track(&t), None);
    }

    #[test]
    fn pick_from_search_prefers_synced_with_matching_duration() {
        let wrong = track(Some("[00:01.00] wrong"), None, false, Some(100.0));
        let right = track(Some("[00:01.00] right"), None, false, Some(201.0));
        assert_eq!(
            pick_from_search(&[wrong, right], Some(200)).as_deref(),
            Some("[00:01.00] right")
        );
    }

    #[test]
    fn pick_from_search_synced_any_beats_plain() {
        let plain = track(None, Some("plain"), false, Some(200.0));
        let synced = track(Some(SYNCED), None, false, Some(999.0));
        assert_eq!(
            pick_from_search(&[plain, synced], Some(200)).as_deref(),
            Some(SYNCED)
        );
    }

    #[test]
    fn pick_from_search_plain_prefers_matching_duration() {
        let wrong = track(None, Some("wrong"), false, Some(100.0));
        let right = track(None, Some("right"), false, Some(200.0));
        assert_eq!(
            pick_from_search(&[wrong, right], Some(200)).as_deref(),
            Some("right")
        );
    }

    #[test]
    fn pick_from_search_instrumental_only_when_nothing_else() {
        let instr = track(None, None, true, None);
        assert_eq!(
            pick_from_search(&[instr], None).as_deref(),
            Some(INSTRUMENTAL_LYRICS)
        );

        let instr = track(None, None, true, None);
        let plain = track(None, Some("plain"), false, None);
        assert_eq!(
            pick_from_search(&[instr, plain], None).as_deref(),
            Some("plain")
        );
    }

    #[test]
    fn pick_from_search_empty_slice_gives_none() {
        assert_eq!(pick_from_search(&[], None), None);
        assert_eq!(pick_from_search(&[], Some(200)), None);
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    fn hit(name: &str, artist: &str) -> LrclibTrack {
        LrclibTrack {
            track_name: Some(name.into()),
            artist_name: Some(artist.into()),
            plain_lyrics: Some("lyrics".into()),
            ..Default::default()
        }
    }

    #[test]
    fn search_hits_for_other_songs_are_dropped() {
        let hits = vec![
            hit("Widow's Walk", "Larkin Poe"),
            hit("Widow", "Christopher Larkin"),
        ];
        let kept = filter_matching(hits, "Christopher Larkin", "Widow");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].artist_name.as_deref(), Some("Christopher Larkin"));
        assert!(filter_matching(vec![hit("Other", "Someone")], "Christopher Larkin", "Widow").is_empty());
    }
}
