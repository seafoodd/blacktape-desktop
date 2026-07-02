use crate::search::bandcamp::ItemType;
use crate::types::Platform;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Serialize, Clone)]
pub struct SearchSuggestion {
    pub item_type: ItemType,
    pub name: String,
    pub band_name: String,
    pub album_name: Option<String>,
    pub item_url_path: String,
    pub img: String,

    pub subscriber_count: Option<u64>,
    pub view_count: Option<u64>,
    pub year: Option<i32>,
    pub duration: Option<u32>,
    pub platform: Platform,
}

pub mod bandcamp;
pub mod youtube;

/// Processes raw suggestions gathered from all platforms into a single, cohesive, sorted feed.
pub fn process_raw_results(raw_items: Vec<SearchSuggestion>, query: &str) -> Vec<SearchSuggestion> {
    let query_lower = query.trim().to_lowercase();

    // 1. Separate items from all platforms into their respective categories
    let mut tracks = Vec::new();
    let mut albums = Vec::new();
    let mut artists = Vec::new();

    for item in raw_items {
        match item.item_type {
            ItemType::Track => tracks.push(item),
            ItemType::Album => albums.push(item),
            ItemType::Artist => artists.push(item),
            ItemType::Unknown => {}
        }
    }

    // 2. Score each category using our cross-platform scoring algorithm
    let mut scored_tracks: Vec<(f32, SearchSuggestion)> = tracks
        .into_iter()
        .enumerate()
        .map(|(i, t)| (calculate_relevance(&t, &query_lower, i, 0.0), t))
        .collect();

    let mut scored_albums: Vec<(f32, SearchSuggestion)> = albums
        .into_iter()
        .enumerate()
        .map(|(i, a)| (calculate_relevance(&a, &query_lower, i, 10.0), a))
        .collect();

    let mut scored_artists: Vec<(f32, SearchSuggestion)> = artists
        .into_iter()
        .enumerate()
        .map(|(i, a)| (calculate_relevance(&a, &query_lower, i, 20.0), a))
        .collect();

    // 3. Sort individual categories by score descending.
    // If scores tie, explicitly prioritize Bandcamp for quality.
    let sort_desc = |a: &(f32, SearchSuggestion), b: &(f32, SearchSuggestion)| {
        b.0.partial_cmp(&a.0)
            .unwrap_or_else(|| match (&a.1.platform, &b.1.platform) {
                (Platform::Bandcamp, Platform::Youtube) => std::cmp::Ordering::Less,
                (Platform::Youtube, Platform::Bandcamp) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    };
    scored_tracks.sort_by(sort_desc);
    scored_albums.sort_by(sort_desc);
    scored_artists.sort_by(sort_desc);

    let mut final_suggestions = Vec::new();
    let mut track_iter = scored_tracks.into_iter().peekable();
    let mut album_iter = scored_albums.into_iter().peekable();
    let mut artist_iter = scored_artists.into_iter().peekable();

    // --- Suffix-Aware Cross-Platform Deduplication ---
    let mut seen_items = HashSet::new();
    let get_dedup_key = |item: &SearchSuggestion| {
        let type_str = match item.item_type {
            ItemType::Artist => "artist",
            ItemType::Track => "track",
            ItemType::Album => "album",
            _ => "unknown",
        };

        let mut clean_name = item.name.to_lowercase();
        if let Some(idx) = clean_name.find('(') {
            clean_name.truncate(idx);
        }
        if let Some(idx) = clean_name.find('[') {
            clean_name.truncate(idx);
        }
        if let Some(idx) = clean_name.find(" - ") {
            clean_name.truncate(idx);
        }
        let normalized_name = clean_name.trim();

        // Key excludes platform, allowing cross-platform deduplication matches!
        format!(
            "{}:{}:{}",
            type_str,
            normalized_name,
            item.band_name.to_lowercase()
        )
    };

    // 4. Find Champion (Slot #1)
    let mut champion_type = None;
    let mut max_score = -1000.0;

    if let Some((score, _)) = artist_iter.peek() {
        if *score > max_score {
            max_score = *score;
            champion_type = Some(ItemType::Artist);
        }
    }
    if let Some((score, _)) = track_iter.peek() {
        if *score > max_score {
            max_score = *score;
            champion_type = Some(ItemType::Track);
        }
    }
    if let Some((score, _)) = album_iter.peek() {
        if *score > max_score {
            max_score = *score;
            champion_type = Some(ItemType::Album);
        }
    }

    if let Some(t) = champion_type {
        let champ = match t {
            ItemType::Artist => artist_iter.next().map(|(_, item)| item),
            ItemType::Track => track_iter.next().map(|(_, item)| item),
            ItemType::Album => album_iter.next().map(|(_, item)| item),
            _ => None,
        };
        if let Some(item) = champ {
            seen_items.insert(get_dedup_key(&item));
            final_suggestions.push(item);
        }
    }

    // 5. Fill Track Slots (Up to 3 unique tracks)
    while final_suggestions
        .iter()
        .filter(|i| i.item_type == ItemType::Track)
        .count()
        < 3
    {
        if let Some((_, track)) = track_iter.next() {
            let key = get_dedup_key(&track);
            if seen_items.insert(key) {
                final_suggestions.push(track);
            }
        } else {
            break;
        }
    }

    // 6. Fill Album Slots (Up to 2 unique albums)
    while final_suggestions
        .iter()
        .filter(|i| i.item_type == ItemType::Album)
        .count()
        < 2
    {
        if let Some((_, album)) = album_iter.next() {
            let key = get_dedup_key(&album);
            if seen_items.insert(key) {
                final_suggestions.push(album);
            }
        } else {
            break;
        }
    }

    // 7. Merge and Sweep Remainder Feed
    let mut remainder: Vec<(f32, SearchSuggestion)> =
        track_iter.chain(album_iter).chain(artist_iter).collect();
    remainder.sort_by(sort_desc);

    for (_, item) in remainder {
        let key = get_dedup_key(&item);
        if seen_items.insert(key) {
            final_suggestions.push(item);
        }
    }

    final_suggestions
}

fn calculate_relevance(
    item: &SearchSuggestion,
    query_lower: &str,
    native_index: usize,
    type_bias: f32,
) -> f32 {
    let name_lower = item.name.to_lowercase();
    let band_lower = item.band_name.to_lowercase();

    let mut score = 30.0 / (native_index as f32 + 1.0).sqrt();
    score += type_bias;

    // --- THE PLATFORM QUALITY BALANCER ---
    // YouTube view metrics can easily add up to +50 points. Bandcamp items do not have views.
    // We add a solid baseline to Bandcamp items so they can survive head-to-head score metrics,
    // ensuring the high-quality source takes the top slot if the metadata matches.
    if item.platform == Platform::Bandcamp {
        score += 25.0;
    }

    let mut popularity_bonus = 0.0;
    if let Some(views) = item.view_count {
        if views > 0 {
            popularity_bonus += (views as f32).ln() * 3.0;
        }
    }
    if let Some(subs) = item.subscriber_count {
        if subs > 0 {
            popularity_bonus += (subs as f32).ln() * 3.5;
        }
    }
    score += popularity_bonus;

    if item.item_type == ItemType::Artist && name_lower == query_lower {
        if item.subscriber_count.unwrap_or(0) > 100_000 {
            score += 150.0;
        }
    }

    if name_lower == query_lower {
        score += 80.0;
    } else if name_lower.starts_with(query_lower) {
        score += 35.0;
    } else if name_lower.contains(query_lower) {
        score += 15.0;
    } else if query_lower.contains(&name_lower) && !name_lower.is_empty() {
        score += 55.0;
    } else {
        let title_fuzzy = strsim::jaro_winkler(&name_lower, query_lower) as f32;
        if title_fuzzy > 0.85 {
            score += (title_fuzzy - 0.85) * 333.3;
        }
    }

    if band_lower == query_lower {
        score += 90.0;
        if item.item_type == ItemType::Album {
            score += 45.0;
        }
    } else if band_lower.starts_with(query_lower) {
        score += 40.0;
    } else if band_lower.contains(query_lower) {
        score += 15.0;
    } else {
        let band_fuzzy = strsim::jaro_winkler(&band_lower, query_lower) as f32;
        if band_fuzzy > 0.85 {
            score += (band_fuzzy - 0.85) * 333.3;
        }
    }

    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    if query_words.len() > 1 {
        let mut matched_words = 0;
        let mut matches_title = false;
        let mut matches_band = false;

        for word in &query_words {
            let mut word_matched = false;
            if name_lower.contains(word) {
                matches_title = true;
                word_matched = true;
            }
            if band_lower.contains(word) {
                matches_band = true;
                word_matched = true;
            }
            if word_matched {
                matched_words += 1;
            }
        }
        let token_match_ratio = matched_words as f32 / query_words.len() as f32;
        score += token_match_ratio * 20.0;

        if matches_title && matches_band && item.item_type != ItemType::Artist {
            score += 130.0;
        }
    }

    if let Some(duration_secs) = item.duration {
        if duration_secs < 45 || duration_secs > 900 {
            score -= 30.0;
        }
    }

    score
}
