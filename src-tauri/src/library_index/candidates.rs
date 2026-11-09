//! Finding the platform album of a library album that has no stored source (added before the missing songs
//! feature): search the platforms, rank the album results by how well they match, let the user pick one.
//! The ranking is pure; the command only gathers the search results.

use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use serde::Serialize;
use std::collections::HashSet;
use tokio::task::JoinSet;

/// Best candidates shown to the user.
const MAX_CANDIDATES: usize = 5;
/// A candidate needs at least this combined score and a title that is at least `MIN_TITLE` similar.
const MIN_SCORE: f32 = 0.5;
const MIN_TITLE: f32 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlbumCandidate {
    pub platform: Platform,
    pub url: String,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub year: Option<i32>,
    pub score: f32,
}

/// Lowercase words only: text in (), [] removed ("(Deluxe Edition)"), punctuation turned into spaces.
pub fn normalize_text(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in text.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            c if c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 0..=1: 1 when equal after normalizing, 0.8 when one contains the other, else the share of common words.
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (normalize_text(a), normalize_text(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.contains(&b) || b.contains(&a) {
        return 0.8;
    }
    let wa: HashSet<&str> = a.split(' ').collect();
    let wb: HashSet<&str> = b.split(' ').collect();
    let common = wa.intersection(&wb).count() as f32;
    let total = wa.union(&wb).count() as f32;
    common / total
}

/// Score of a search result for the library album, None when it is not an album or too different.
pub fn score_candidate(album_artist: &str, album: &str, item: &SearchSuggestion) -> Option<f32> {
    if item.item_type != ItemType::Album {
        return None;
    }
    let title = similarity(album, &item.name);
    let artist = similarity(album_artist, &item.band_name);
    let score = 0.7 * title + 0.3 * artist;
    (title >= MIN_TITLE && score >= MIN_SCORE).then_some(score)
}

/// The best albums among `items`, best first, at most `MAX_CANDIDATES`, one per url.
pub fn rank_candidates(album_artist: &str, album: &str, items: Vec<SearchSuggestion>) -> Vec<AlbumCandidate> {
    let mut seen = HashSet::new();
    let mut ranked: Vec<AlbumCandidate> = items
        .into_iter()
        .filter_map(|item| {
            let score = score_candidate(album_artist, album, &item)?;
            if item.item_url_path.is_empty() || !seen.insert(item.item_url_path.clone()) {
                return None;
            }
            Some(AlbumCandidate {
                platform: item.platform,
                url: item.item_url_path,
                title: item.name,
                artist: item.band_name,
                cover_url: Some(item.img).filter(|i| !i.is_empty()),
                year: item.year,
                score,
            })
        })
        .collect();
    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(MAX_CANDIDATES);
    ranked
}

/// Platform albums that may be the library album (`album_artist`, `album`), best first.
#[tauri::command]
pub async fn find_album_candidates(album_artist: String, album: String) -> Vec<AlbumCandidate> {
    let query = format!("{album_artist} {album}");
    let mut tasks: JoinSet<Vec<SearchSuggestion>> = JoinSet::new();
    for platform in [Platform::Bandcamp, Platform::Youtube, Platform::Soundcloud] {
        let q = query.clone();
        tasks.spawn(async move {
            let result = match platform {
                Platform::Bandcamp => crate::search::bandcamp::search(&q).await,
                Platform::Youtube => crate::search::search_youtube_with_fallback(&q).await,
                _ => crate::search::soundcloud::search(&q).await,
            };
            result.unwrap_or_else(|e| {
                eprintln!("[library_index] candidate search failed: {e}");
                Vec::new()
            })
        });
    }
    let mut items = Vec::new();
    while let Some(res) = tasks.join_next().await {
        if let Ok(found) = res {
            items.extend(found);
        }
    }
    rank_candidates(&album_artist, &album, items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(item_type: ItemType, name: &str, band: &str, url: &str, img: &str) -> SearchSuggestion {
        SearchSuggestion {
            item_type,
            name: name.to_string(),
            band_name: band.to_string(),
            album_name: None,
            item_url_path: url.to_string(),
            img: img.to_string(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Bandcamp,
        }
    }

    fn album(name: &str, band: &str, url: &str) -> SearchSuggestion {
        item(ItemType::Album, name, band, url, "cover.jpg")
    }

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn normalize_drops_bracketed_edition_text() {
        assert_eq!(normalize_text("Abbey Road (Deluxe Edition)"), "abbey road");
        assert_eq!(normalize_text("Mezzanine [Remastered]"), "mezzanine");
    }

    #[test]
    fn normalize_turns_punctuation_into_spaces_and_lowercases() {
        assert_eq!(normalize_text("Hello, World! - It's"), "hello world it s");
        assert_eq!(normalize_text("  SPACED   out  "), "spaced out");
    }

    #[test]
    fn similarity_equal_after_normalizing_is_one() {
        assert!(approx(similarity("Abbey Road", "abbey road"), 1.0));
        assert!(approx(similarity("Abbey Road (Deluxe Edition)", "Abbey Road"), 1.0));
    }

    #[test]
    fn similarity_containment_is_point_eight() {
        assert!(approx(similarity("Abbey Road", "Abbey Road Live"), 0.8));
        assert!(approx(similarity("Abbey Road Live", "Abbey Road"), 0.8));
    }

    #[test]
    fn similarity_unrelated_is_zero() {
        assert!(approx(similarity("Abbey Road", "Zebra"), 0.0));
    }

    #[test]
    fn similarity_empty_is_zero() {
        assert!(approx(similarity("", "Abbey Road"), 0.0));
        assert!(approx(similarity("Abbey Road", ""), 0.0));
        // Only bracketed text normalizes to nothing.
        assert!(approx(similarity("(Deluxe Edition)", "Abbey Road"), 0.0));
    }

    #[test]
    fn similarity_word_overlap_is_between_zero_and_one() {
        // Common {red, blue} out of union {red, blue, green, yellow}.
        let s = similarity("red blue green", "red blue yellow");
        assert!(s > 0.0 && s < 1.0, "expected overlap strictly between 0 and 1, got {s}");
        assert!(approx(s, 0.5));
    }

    #[test]
    fn score_is_none_for_track_and_artist_items() {
        let track = item(ItemType::Track, "Abbey Road", "The Beatles", "https://x/t", "");
        let artist = item(ItemType::Artist, "The Beatles", "The Beatles", "https://x/a", "");
        assert_eq!(score_candidate("The Beatles", "Abbey Road", &track), None);
        assert_eq!(score_candidate("The Beatles", "Abbey Road", &artist), None);
    }

    #[test]
    fn score_is_none_for_unrelated_title() {
        let other = album("Zebra", "The Beatles", "https://x/z");
        assert_eq!(score_candidate("The Beatles", "Abbey Road", &other), None);
    }

    #[test]
    fn score_is_one_for_exact_title_and_artist() {
        let exact = album("Abbey Road", "The Beatles", "https://x/abbey");
        let score = score_candidate("The Beatles", "Abbey Road", &exact).expect("exact match should score");
        assert!(approx(score, 1.0), "score was {score}");
    }

    #[test]
    fn score_is_some_for_same_title_with_different_artist() {
        // title 1.0 * 0.7 + artist 0.0 * 0.3 = 0.7, which passes both thresholds.
        let cover = album("Abbey Road", "Some Cover Band", "https://x/cover");
        let score = score_candidate("The Beatles", "Abbey Road", &cover).expect("same title should still score");
        assert!(approx(score, 0.7), "score was {score}");
    }

    #[test]
    fn rank_sorts_best_first_drops_non_matching_and_dedupes_by_url() {
        let items = vec![
            album("Abbey Road Live", "The Beatles", "https://x/live"),
            album("Abbey Road", "The Beatles", "https://x/abbey"),
            album("Zebra", "The Beatles", "https://x/zebra"),
            item(ItemType::Track, "Abbey Road", "The Beatles", "https://x/track", ""),
            album("Abbey Road", "The Beatles", "https://x/abbey"),
            album("Abbey Road", "The Beatles", ""),
        ];
        let ranked = rank_candidates("The Beatles", "Abbey Road", items);
        let urls: Vec<&str> = ranked.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(urls, vec!["https://x/abbey", "https://x/live"]);
        assert!(ranked[0].score > ranked[1].score);
        assert!(approx(ranked[0].score, 1.0));
    }

    #[test]
    fn rank_caps_at_five_candidates() {
        let items: Vec<SearchSuggestion> = (0..8)
            .map(|i| album("Abbey Road", "The Beatles", &format!("https://x/{i}")))
            .collect();
        let ranked = rank_candidates("The Beatles", "Abbey Road", items);
        assert_eq!(ranked.len(), 5);
        assert!(ranked.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn rank_maps_empty_img_to_no_cover_url() {
        let items = vec![
            item(ItemType::Album, "Abbey Road", "The Beatles", "https://x/none", ""),
            item(ItemType::Album, "Abbey Road", "The Beatles", "https://x/some", "cover.jpg"),
        ];
        let ranked = rank_candidates("The Beatles", "Abbey Road", items);
        let none = ranked.iter().find(|c| c.url == "https://x/none").expect("candidate kept");
        let some = ranked.iter().find(|c| c.url == "https://x/some").expect("candidate kept");
        assert_eq!(none.cover_url, None);
        assert_eq!(some.cover_url.as_deref(), Some("cover.jpg"));
    }
}
