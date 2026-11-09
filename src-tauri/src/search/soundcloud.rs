//! SoundCloud search (tracks, sets, users) mapped to [`SearchSuggestion`].
//!
//! Self-contained so it can feed both the dropdown (`search`) and a paged full-search
//! page (`search_kind`).

use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::soundcloud::{
    self, availability, next_offset_from_href, playlist_artwork, track_artwork, year_from_dates,
    Availability, ScCollection, ScPlaylist, ScTrack, ScUser,
};
use crate::types::Platform;
use std::error::Error;

const DROPDOWN_TRACKS: u32 = 20;
const DROPDOWN_SETS: u32 = 10;
const DROPDOWN_USERS: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Tracks,
    /// Albums, EPs and playlists ("sets").
    Sets,
    Users,
}

impl SearchKind {
    fn path(self) -> &'static str {
        match self {
            SearchKind::Tracks => "/search/tracks",
            SearchKind::Sets => "/search/playlists",
            SearchKind::Users => "/search/users",
        }
    }
}

/// One page of results plus the offset of the next page, if there is one.
/// The paging fields are for the full search page (not on `master` yet).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SearchPage {
    pub items: Vec<SearchSuggestion>,
    pub next_offset: Option<u32>,
    pub total_results: Option<u64>,
}

/// Dropdown search: tracks, sets and users in parallel. Fails only if all three fail.
pub async fn search(query: &str) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let (tracks, sets, users) = tokio::join!(
        search_kind(query, SearchKind::Tracks, DROPDOWN_TRACKS, 0),
        search_kind(query, SearchKind::Sets, DROPDOWN_SETS, 0),
        search_kind(query, SearchKind::Users, DROPDOWN_USERS, 0),
    );

    let mut items = Vec::new();
    let mut errors = Vec::new();
    for res in [tracks, sets, users] {
        match res {
            Ok(page) => items.extend(page.items),
            Err(e) => errors.push(e),
        }
    }
    if items.is_empty() && errors.len() == 3 {
        return Err(errors.remove(0).into());
    }
    for e in errors {
        eprintln!("[SoundCloud search] partial failure: {e}");
    }
    Ok(items)
}

/// One page of one result kind. `offset` comes from a previous page's `next_offset`.
pub async fn search_kind(
    query: &str,
    kind: SearchKind,
    limit: u32,
    offset: u32,
) -> Result<SearchPage, String> {
    let page: ScCollection = soundcloud::api_get(
        kind.path(),
        &[
            ("q", query.to_string()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
            ("linked_partitioning", "1".to_string()),
        ],
    )
    .await?;
    Ok(map_page(&page, kind))
}

pub fn map_page(page: &ScCollection, kind: SearchKind) -> SearchPage {
    let items = match kind {
        SearchKind::Tracks => page
            .items::<ScTrack>()
            .iter()
            .filter_map(track_suggestion)
            .collect(),
        SearchKind::Sets => page
            .items::<ScPlaylist>()
            .iter()
            .filter_map(playlist_suggestion)
            .collect(),
        SearchKind::Users => page
            .items::<ScUser>()
            .iter()
            .filter_map(user_suggestion)
            .collect(),
    };
    SearchPage {
        items,
        next_offset: next_offset_from_href(page.next_href.as_deref()),
        total_results: page.total_results,
    }
}

/// Tracks that can't be downloaded in full (Go+ previews, geo-blocked, not streamable)
/// are left out: offering them would only lead to a failed download.
pub fn track_suggestion(track: &ScTrack) -> Option<SearchSuggestion> {
    if availability(track) != Availability::Full {
        return None;
    }
    let url = track.permalink_url.clone()?;
    let (artist, title) = soundcloud::artist_and_title(track);
    Some(SearchSuggestion {
        item_type: ItemType::Track,
        name: title,
        band_name: artist,
        album_name: track
            .publisher_metadata
            .as_ref()
            .and_then(|p| p.album_title.clone())
            .filter(|a| !a.is_empty()),
        item_url_path: url,
        img: track_artwork(track).unwrap_or_default(),
        subscriber_count: None,
        view_count: track.playback_count,
        year: year_from_dates(&[track.release_date.as_deref()]),
        duration: track
            .full_duration
            .or(track.duration)
            .map(|ms| (ms / 1000) as u32),
        platform: Platform::Soundcloud,
    })
}

pub fn playlist_suggestion(playlist: &ScPlaylist) -> Option<SearchSuggestion> {
    let url = playlist.permalink_url.clone()?;
    if playlist.track_count == Some(0) {
        return None;
    }
    Some(SearchSuggestion {
        item_type: ItemType::Album,
        name: playlist.title.clone(),
        band_name: playlist
            .user
            .as_ref()
            .map(|u| u.username.clone())
            .unwrap_or_else(|| "Unknown Artist".into()),
        album_name: None,
        item_url_path: url,
        img: playlist_artwork(playlist).unwrap_or_default(),
        subscriber_count: None,
        view_count: None,
        year: year_from_dates(&[
            playlist.release_date.as_deref(),
            playlist.published_at.as_deref(),
            playlist.display_date.as_deref(),
        ]),
        duration: None,
        platform: Platform::Soundcloud,
    })
}

pub fn user_suggestion(user: &ScUser) -> Option<SearchSuggestion> {
    let url = user.permalink_url.clone()?;
    Some(SearchSuggestion {
        item_type: ItemType::Artist,
        name: user.username.clone(),
        band_name: "Artist".to_string(),
        album_name: None,
        item_url_path: url,
        img: user
            .avatar_url
            .as_deref()
            .map(soundcloud::upgrade_artwork_url)
            .unwrap_or_default(),
        subscriber_count: user.followers_count,
        view_count: None,
        year: None,
        duration: None,
        platform: Platform::Soundcloud,
    })
}
