use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use rustypipe::client::RustyPipe;
use rustypipe::model::MusicItem;
use rustypipe::param::search_filter::MusicSearchFilter;
use std::collections::HashSet;
use std::error::Error;
use tokio::try_join;

impl From<MusicItem> for SearchSuggestion {
    fn from(item: MusicItem) -> Self {
        println!("MusicItem {:?}", item);

        match item {
            MusicItem::Track(t) => SearchSuggestion {
                item_type: ItemType::Track,
                name: t.name,
                band_name: t
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<String>>()
                    .join(", "),
                album_name: t.album.map(|a| a.name),
                item_url_path: format!("https://youtube.com/watch?v={}", t.id),
                img: t.cover.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: None,
                view_count: t.view_count,
                year: None,
                duration: t.duration,
                platform: Platform::Youtube,
            },
            MusicItem::Artist(a) => SearchSuggestion {
                item_type: ItemType::Artist,
                name: a.name,
                band_name: "Artist".to_string(),
                album_name: None,
                item_url_path: format!("https://youtube.com/browse/{}", a.id),
                img: a.avatar.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: a.subscriber_count,
                view_count: None,
                year: None,
                duration: None,
                platform: Platform::Youtube,
            },
            MusicItem::Album(a) => SearchSuggestion {
                item_type: ItemType::Album,
                name: a.name,
                band_name: a
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<String>>()
                    .join(", "),
                album_name: None,
                item_url_path: format!("https://music.youtube.com/browse/{}", a.id),
                img: a.cover.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: None,
                view_count: None,
                year: a.year.map(|y| y as i32),
                duration: None,
                platform: Platform::Youtube,
            },
            _ => todo!("Handle other MusicItem variants"),
        }
    }
}

pub struct CombinedResults {
    pub tracks: Vec<SearchSuggestion>,
    pub albums: Vec<SearchSuggestion>,
    pub artists: Vec<SearchSuggestion>,
}

async fn search_all_categories(
    query: &str,
) -> Result<CombinedResults, Box<dyn Error + Send + Sync>> {
    let rp = RustyPipe::new();

    let track_query = rp.query();
    let album_query = rp.query();
    let artist_query = rp.query();

    let (track_res, album_res, artist_res) = try_join!(
        track_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Tracks)),
        album_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Albums)),
        artist_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Artists))
    )?;

    Ok(CombinedResults {
        tracks: track_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from)
            .collect(),
        albums: album_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from)
            .collect(),
        artists: artist_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from)
            .collect(),
    })
}

pub async fn search(query: String) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let results = search_all_categories(&query).await?;
    let query_lower = query.trim().to_lowercase();

    // 1. Score each category independently
    let mut scored_tracks: Vec<(f32, SearchSuggestion)> = results
        .tracks
        .into_iter()
        .enumerate()
        .map(|(i, t)| (calculate_relevance(&t, &query_lower, i, 0.0), t))
        .collect();

    let mut scored_albums: Vec<(f32, SearchSuggestion)> = results
        .albums
        .into_iter()
        .enumerate()
        .map(|(i, a)| (calculate_relevance(&a, &query_lower, i, 10.0), a))
        .collect();

    let mut scored_artists: Vec<(f32, SearchSuggestion)> = results
        .artists
        .into_iter()
        .enumerate()
        .map(|(i, a)| (calculate_relevance(&a, &query_lower, i, 20.0), a))
        .collect();

    // 2. Sort individual categories by score descending
    let sort_desc = |a: &(f32, SearchSuggestion), b: &(f32, SearchSuggestion)| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
    };
    scored_tracks.sort_by(sort_desc);
    scored_albums.sort_by(sort_desc);
    scored_artists.sort_by(sort_desc);

    // Convert arrays into peekable iterators for dynamic slot allocation
    let mut final_suggestions = Vec::new();
    let mut track_iter = scored_tracks.into_iter().peekable();
    let mut album_iter = scored_albums.into_iter().peekable();
    let mut artist_iter = scored_artists.into_iter().peekable();

    // --- Advanced Suffix-Aware Deduplication ---
    let mut seen_items = HashSet::new();
    let get_dedup_key = |item: &SearchSuggestion| {
        let type_str = match item.item_type {
            ItemType::Artist => "artist",
            ItemType::Track => "track",
            ItemType::Album => "album",
            _ => "unknown",
        };

        // Strips out trailing parentheticals/brackets/remaster tags to group variations together
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

        format!(
            "{}:{}:{}",
            type_str,
            normalized_name,
            item.band_name.to_lowercase()
        )
    };

    // 3. Find the undisputed "Top Result" Champion
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

    // Pull the champion from its iterator and lock it into Slot #1
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

    // 4. Fill Track Slots (Ensures up to 3 unique tracks populate the initial layout block)
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

    // 5. Fill Album Slots (Ensures up to 2 unique albums populate the initial layout block)
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

    // 6. Merge remaining items, sweep duplicates, and sort purely by score for the bottom feed
    let mut remainder: Vec<(f32, SearchSuggestion)> =
        track_iter.chain(album_iter).chain(artist_iter).collect();

    remainder.sort_by(sort_desc);

    for (_, item) in remainder {
        let key = get_dedup_key(&item);
        if seen_items.insert(key) {
            final_suggestions.push(item);
        }
    }

    Ok(final_suggestions)
}

fn calculate_relevance(
    item: &SearchSuggestion,
    query_lower: &str,
    native_index: usize,
    type_bias: f32,
) -> f32 {
    let name_lower = item.name.to_lowercase();
    let band_lower = item.band_name.to_lowercase();

    // 1. Gentle Native Rank Decay
    let mut score = 30.0 / (native_index as f32 + 1.0).sqrt();
    score += type_bias;

    // 2. Popularity Scaling
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

    // 3. Exact Multi-Million Artist Protection Rule
    if item.item_type == ItemType::Artist && name_lower == query_lower {
        if item.subscriber_count.unwrap_or(0) > 100_000 {
            score += 150.0;
        }
    }

    // 4. Primary Text Matching (Item Title vs Query)
    if name_lower == query_lower {
        score += 80.0;
    } else if name_lower.starts_with(query_lower) {
        score += 35.0;
    } else if name_lower.contains(query_lower) {
        score += 15.0;
    } else if query_lower.contains(&name_lower) && !name_lower.is_empty() {
        // Fixes "Nirvana In Utero" issue: Bumps the item if the query explicitly incorporates its title
        score += 55.0;
    } else {
        // TYPO SAFETY NET: Only apply fuzzy matching if structural matches completely failed.
        // This ensures "Live" doesn't accidentally match "Love".
        let title_fuzzy = strsim::jaro_winkler(&name_lower, query_lower) as f32;

        // 0.85 is the typical threshold for a near-certain typo (e.g., "nirvna" vs "nirvana")
        if title_fuzzy > 0.85 {
            // Scale the bonus smoothly up to 50 points for a near-perfect match
            score += (title_fuzzy - 0.85) * 333.3;
        }
    }

    // 5. Artist/Band Context Matching
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
        // Catch typos in the band name as well
        let band_fuzzy = strsim::jaro_winkler(&band_lower, query_lower) as f32;
        if band_fuzzy > 0.85 {
            score += (band_fuzzy - 0.85) * 333.3;
        }
    }

    // 6. Tokenized Multi-word & Cross-Intent Overrides
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

        // Cross-Intent Override: If the multi-word query captures BOTH the artist name and track/album name,
        // we scale up the specific release so it organically takes Slot #1 over a generic artist profile.
        if matches_title && matches_band && item.item_type != ItemType::Artist {
            score += 130.0;
        }
    }

    // 7. Track Duration Sanity Filter
    if let Some(duration_secs) = item.duration {
        if duration_secs < 45 || duration_secs > 900 {
            score -= 30.0;
        }
    }

    score
}
