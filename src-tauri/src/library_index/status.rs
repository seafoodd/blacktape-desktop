//! Download status of remote items against the library. Owner: part A.
//! No persistent in-memory cache (D4): `LibraryIndex` is rebuilt per command call from one cheap SELECT.

use crate::db::album_sources::SongSourceRef;
use crate::library_index::normalize::{item_id_key, normalize_url_key};
use crate::types::Platform;
use serde::{Deserialize, Serialize};
use crate::utils::make_canonical_slug;
use std::collections::HashSet;

/// A remote item the frontend asks about.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteRef {
    pub platform: Platform,
    /// Page url of the track or album, exactly as shown in the UI (also the key of the answer map).
    pub url: String,
    /// Artist and title of a TRACK. Songs downloaded before the library stored page urls cannot be matched by
    /// url, so a track also counts as downloaded when the library has a song with the same artist + title slug.
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    None,
    Partial,
    Downloaded,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AlbumStatus {
    pub state: DownloadState,
    pub owned: usize,
    pub total: usize,
}

/// Set of normalized keys of every library song (url keys and youtube item-id keys).
#[derive(Debug, Default)]
pub struct LibraryIndex {
    pub keys: HashSet<String>,
    /// `canonical_track_slug` of every library song (artist + title), also songs without a remote reference.
    pub slugs: HashSet<String>,
}

impl LibraryIndex {
    pub fn from_rows(rows: &[SongSourceRef]) -> Self {
        let mut keys = HashSet::with_capacity(rows.len() * 2);
        for row in rows {
            if let Some(key) = row.source_url.as_deref().and_then(normalize_url_key) {
                keys.insert(key);
            }
            if let (Some(id), Some(platform)) =
                (row.source_item_id.as_deref(), platform_from_str(&row.source))
            {
                if let Some(key) = item_id_key(platform, id) {
                    keys.insert(key);
                }
            }
        }
        Self { keys, slugs: HashSet::new() }
    }

    /// Adds the track slugs of the library (see `contains_by_meta`).
    pub fn with_slugs(mut self, slugs: impl IntoIterator<Item = String>) -> Self {
        self.slugs.extend(slugs.into_iter().filter(|s| !s.is_empty()));
        self
    }

    /// True when the library has a song with this artist and title (slug of `artist title`). A track credited to
    /// several artists ("A, B") is also tried with its first artist, which is how the library files it.
    pub fn contains_by_meta(&self, artist: &str, title: &str) -> bool {
        if self.slugs.is_empty() || title.trim().is_empty() {
            return false;
        }
        let first = artist.split(',').next().unwrap_or(artist);
        [artist, first]
            .iter()
            .filter(|a| !a.trim().is_empty())
            .any(|a| self.slugs.contains(&make_canonical_slug(a, title)))
    }

    /// True when the remote track is in the library (by url key, else by youtube item id).
    pub fn contains_track(&self, platform: Platform, url: &str) -> bool {
        let Some(key) = normalize_url_key(url) else {
            return false;
        };
        if self.keys.contains(&key) {
            return true;
        }
        if platform == Platform::Youtube {
            if let Some(id) = key.strip_prefix("yt:") {
                return self.keys.contains(&format!("youtube:{id}"));
            }
        }
        false
    }
}

/// none / partial / downloaded from the counts. `total == 0` -> None. Pure.
pub fn album_state(owned: usize, total: usize) -> DownloadState {
    if total == 0 || owned == 0 {
        DownloadState::None
    } else if owned >= total {
        DownloadState::Downloaded
    } else {
        DownloadState::Partial
    }
}

/// Parses the lowercase platform name stored in `songs.source` / `album_sources.platform`.
pub fn platform_from_str(value: &str) -> Option<Platform> {
    match value.trim().to_ascii_lowercase().as_str() {
        "youtube" => Some(Platform::Youtube),
        "bandcamp" => Some(Platform::Bandcamp),
        "soundcloud" => Some(Platform::Soundcloud),
        "local" => Some(Platform::Local),
        _ => None,
    }
}

/// Lowercase platform name as stored in the database (same as the serde name, D20).
pub fn platform_to_str(platform: Platform) -> &'static str {
    match platform {
        Platform::Youtube => "youtube",
        Platform::Bandcamp => "bandcamp",
        Platform::Soundcloud => "soundcloud",
        Platform::Local => "local",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(source: &str, url: Option<&str>, item_id: Option<&str>) -> SongSourceRef {
        SongSourceRef {
            source: source.to_string(),
            source_url: url.map(str::to_string),
            source_item_id: item_id.map(str::to_string),
        }
    }

    #[test]
    fn album_state_covers_all_cases() {
        assert_eq!(album_state(0, 0), DownloadState::None);
        assert_eq!(album_state(3, 0), DownloadState::None);
        assert_eq!(album_state(0, 5), DownloadState::None);
        assert_eq!(album_state(2, 5), DownloadState::Partial);
        assert_eq!(album_state(5, 5), DownloadState::Downloaded);
        assert_eq!(album_state(7, 5), DownloadState::Downloaded);
        assert_eq!(album_state(1, 1), DownloadState::Downloaded);
    }

    #[test]
    fn from_rows_indexes_normalized_urls() {
        let index = LibraryIndex::from_rows(&[row("bandcamp", Some("https://a.bandcamp.com/track/x/"), None)]);
        assert!(index.contains_track(Platform::Bandcamp, "https://a.bandcamp.com/track/x?from=embed"));
        assert!(!index.contains_track(Platform::Bandcamp, "https://a.bandcamp.com/track/y"));
    }

    #[test]
    fn meta_match_uses_the_artist_title_slug() {
        let index = LibraryIndex::default().with_slugs(["aphex-twin-produk-29-101".to_string(), String::new()]);
        assert!(index.contains_by_meta("Aphex Twin", "produk 29 [101]"));
        assert!(index.contains_by_meta("Aphex Twin, Someone", "produk 29 [101]"), "first artist is tried");
        assert!(!index.contains_by_meta("Other Artist", "produk 29 [101]"));
        assert!(!index.contains_by_meta("Aphex Twin", ""));
        assert!(!LibraryIndex::default().contains_by_meta("Aphex Twin", "x"));
    }

    #[test]
    fn from_rows_skips_empty_and_garbage() {
        let index = LibraryIndex::from_rows(&[
            row("local", None, None),
            row("local", Some(""), Some("")),
            row("bandcamp", Some("garbage"), None),
            row("bandcamp", None, Some("123")),
        ]);
        assert!(index.keys.is_empty());
    }

    #[test]
    fn contains_track_rejects_empty_urls() {
        let index = LibraryIndex::from_rows(&[row("bandcamp", Some("https://a.bandcamp.com/track/x"), None)]);
        assert!(!index.contains_track(Platform::Bandcamp, ""));
        assert!(!index.contains_track(Platform::Bandcamp, "garbage"));
    }

    #[test]
    fn youtube_matches_by_item_id_when_the_url_differs() {
        let index = LibraryIndex::from_rows(&[row("youtube", Some("https://music.youtube.com/something"), Some("abc123"))]);
        assert!(index.contains_track(Platform::Youtube, "https://www.youtube.com/watch?v=abc123"));
        assert!(index.contains_track(Platform::Youtube, "https://youtu.be/abc123"));
        assert!(!index.contains_track(Platform::Youtube, "https://youtu.be/ABC123"));
    }

    #[test]
    fn youtube_matches_by_url_across_hosts() {
        let index = LibraryIndex::from_rows(&[row("youtube", Some("https://music.youtube.com/watch?v=abc123"), None)]);
        assert!(index.contains_track(Platform::Youtube, "https://www.youtube.com/watch?v=abc123&t=5"));
    }

    #[test]
    fn item_ids_of_other_platforms_are_not_indexed() {
        let index = LibraryIndex::from_rows(&[row("bandcamp", None, Some("abc123"))]);
        assert!(index.keys.is_empty());
        assert!(!index.contains_track(Platform::Youtube, "https://youtu.be/abc123"));
    }

    #[test]
    fn unknown_source_does_not_index_the_item_id() {
        let index = LibraryIndex::from_rows(&[row("mystery", None, Some("abc123"))]);
        assert!(index.keys.is_empty());
    }

    #[test]
    fn platform_round_trips() {
        for p in [Platform::Youtube, Platform::Bandcamp, Platform::Soundcloud, Platform::Local] {
            assert_eq!(platform_from_str(platform_to_str(p)), Some(p));
        }
        assert_eq!(platform_from_str(" YouTube "), Some(Platform::Youtube));
        assert_eq!(platform_from_str("spotify"), None);
        assert_eq!(platform_from_str(""), None);
    }

    #[test]
    fn serde_shapes_are_snake_case() {
        assert_eq!(serde_json::to_value(DownloadState::None).unwrap(), "none");
        assert_eq!(serde_json::to_value(DownloadState::Partial).unwrap(), "partial");
        assert_eq!(serde_json::to_value(DownloadState::Downloaded).unwrap(), "downloaded");
        let json = serde_json::to_value(AlbumStatus { state: DownloadState::Partial, owned: 2, total: 5 }).unwrap();
        assert_eq!(json, serde_json::json!({"state": "partial", "owned": 2, "total": 5}));
    }

    #[test]
    fn remote_ref_deserializes_lowercase_platform() {
        let r: RemoteRef = serde_json::from_str(r#"{"platform":"soundcloud","url":"https://x"}"#).unwrap();
        assert_eq!(r.platform, Platform::Soundcloud);
        assert_eq!(r.url, "https://x");
    }
}
