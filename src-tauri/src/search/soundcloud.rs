//! SoundCloud search (tracks, sets, users) mapped to [`SearchSuggestion`].
//!
//! Self-contained so it can feed both the dropdown (`search`) and a paged full-search
//! page (`search_kind`).

use crate::search::bandcamp::ItemType;
use crate::search::full::ArtistProfile;
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
        audience_kind: None,
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
        audience_kind: None,
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
        audience_kind: Some("followers"),
        view_count: None,
        year: None,
        duration: None,
        platform: Platform::Soundcloud,
    })
}

const PROFILE_TRACKS_FETCH: u32 = 50;
const PROFILE_TOP_SONGS: usize = 5;
const PROFILE_SETS_FETCH: u32 = 30;

/// Artist profile of a SoundCloud user url: followers, bio, location, most played tracks and albums / sets.
/// The tracks and sets requests are best effort: the card is still shown without them.
pub async fn artist_profile(url: &str) -> Result<ArtistProfile, String> {
    if !soundcloud::is_soundcloud_url(url) {
        return Err("not a SoundCloud url".to_string());
    }
    let user = match soundcloud::resolve(url).await? {
        soundcloud::ScResource::User(user) => user,
        _ => return Err("not a SoundCloud user".to_string()),
    };
    let tracks_path = format!("/users/{}/tracks", user.id);
    let albums_path = format!("/users/{}/albums", user.id);
    let sets_path = format!("/users/{}/playlists_without_albums", user.id);
    let tracks_params = [("limit", PROFILE_TRACKS_FETCH.to_string())];
    let sets_params = [("limit", PROFILE_SETS_FETCH.to_string())];
    let (tracks, albums, sets) = tokio::join!(
        soundcloud::api_get::<ScCollection>(&tracks_path, &tracks_params),
        soundcloud::api_get::<ScCollection>(&albums_path, &sets_params),
        soundcloud::api_get::<ScCollection>(&sets_path, &sets_params),
    );
    Ok(build_profile(
        &user,
        tracks.ok().map(|c| c.items::<ScTrack>()).unwrap_or_default(),
        albums.ok().map(|c| c.items::<ScPlaylist>()).unwrap_or_default(),
        sets.ok().map(|c| c.items::<ScPlaylist>()).unwrap_or_default(),
    ))
}

/// Pure profile assembly: top songs = the most played downloadable tracks, albums = albums,
/// singles = the user's other sets (EPs / playlists), both as `Album` items.
pub fn build_profile(
    user: &ScUser,
    tracks: Vec<ScTrack>,
    albums: Vec<ScPlaylist>,
    sets: Vec<ScPlaylist>,
) -> ArtistProfile {
    let name = user
        .full_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| user.username.clone());
    let mut top_songs: Vec<SearchSuggestion> = tracks
        .iter()
        .filter_map(track_suggestion)
        .collect();
    top_songs.sort_by_key(|t| std::cmp::Reverse(t.view_count.unwrap_or(0)));
    top_songs.truncate(PROFILE_TOP_SONGS);
    let location = [user.city.as_deref(), user.country_code.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    ArtistProfile {
        platform: Some(Platform::Soundcloud),
        name,
        url: user.permalink_url.clone().unwrap_or_default(),
        avatar: user
            .avatar_url
            .as_deref()
            .map(soundcloud::upgrade_artwork_url),
        banner: None,
        subscribers: None,
        followers: user.followers_count,
        monthly_audience: None,
        description: user
            .description
            .clone()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty()),
        location: (!location.is_empty()).then_some(location),
        top_songs,
        albums: albums.iter().filter_map(playlist_suggestion).collect(),
        singles: sets.iter().filter_map(playlist_suggestion).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> ScUser {
        serde_json::from_str(include_str!("../../fixtures/soundcloud/user_forss.json")).unwrap()
    }

    /// The fixture track is fully downloadable; clones get their own play count and url.
    fn track_with_plays(i: usize, plays: u64) -> ScTrack {
        let mut track: ScTrack =
            serde_json::from_str(include_str!("../../fixtures/soundcloud/track_flickermood.json"))
                .unwrap();
        track.id += i as u64;
        track.playback_count = Some(plays);
        track.permalink_url = Some(format!("https://soundcloud.com/forss/track-{i}"));
        track
    }

    #[test]
    fn build_profile_maps_the_user_and_sorts_top_songs_by_plays() {
        let user = user();
        let tracks = vec![
            track_with_plays(0, 5_000),
            track_with_plays(1, 300),
            track_with_plays(2, 90_000),
        ];

        let profile = build_profile(&user, tracks, vec![], vec![]);

        assert_eq!(profile.platform, Some(Platform::Soundcloud));
        assert!(!profile.name.is_empty());
        assert_eq!(profile.name, user.full_name.clone().unwrap());
        assert_eq!(profile.followers, user.followers_count);
        assert!(profile.top_songs.len() <= PROFILE_TOP_SONGS);
        assert_eq!(profile.top_songs.len(), 3);
        let plays: Vec<Option<u64>> = profile.top_songs.iter().map(|t| t.view_count).collect();
        assert_eq!(plays, vec![Some(90_000), Some(5_000), Some(300)]);
    }

    #[test]
    fn build_profile_keeps_only_the_most_played_top_songs() {
        let tracks = (0..7).map(|i| track_with_plays(i, (i as u64 + 1) * 100)).collect();

        let profile = build_profile(&user(), tracks, vec![], vec![]);

        assert_eq!(profile.top_songs.len(), PROFILE_TOP_SONGS);
        let plays: Vec<Option<u64>> = profile.top_songs.iter().map(|t| t.view_count).collect();
        assert_eq!(plays, vec![Some(700), Some(600), Some(500), Some(400), Some(300)]);
    }
}
