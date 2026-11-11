//! Pure helpers around album sources: building the stored row, reading its track list, matching a library
//! album to its source and computing the missing tracks. No tauri, no database (tests need neither).

use crate::db::album_sources::{AlbumSourceRow, NewAlbumSource, SongSourceRef};
use crate::download::album_preview::{build_preview, AlbumPreview, AlbumTrackInfo};
use crate::download::soundcloud::SkippedTrack;
use crate::download::AlbumDownload;
use crate::library_index::normalize::normalize_url_key;
use crate::library_index::status::{platform_from_str, platform_to_str, LibraryIndex};
use crate::types::Platform;
use html_escape::decode_html_entities;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Downloadable rows of a preview, with the html entities decoded the same way the post-processor decodes the
/// tags (so titles shown for missing tracks look like the ones of the downloaded songs).
fn downloadable_rows(preview: &AlbumPreview) -> Vec<AlbumTrackInfo> {
    preview
        .tracks
        .iter()
        .filter(|t| t.available && !t.url.trim().is_empty())
        .map(|t| AlbumTrackInfo {
            title: decode_html_entities(&t.title).to_string(),
            artists: t.artists.iter().map(|a| decode_html_entities(a).to_string()).collect(),
            ..t.clone()
        })
        .collect()
}

/// The stored row of a preview. `album` / `album_artist` are what the library will contain after the download
/// (`songs.album` / `songs.album_artist`, html entities decoded as `post_processor` does), which is what
/// `match_source` compares. None when the url has no key or the album has no downloadable track.
fn source_from_preview(
    preview: &AlbumPreview,
    album_key: String,
    album_url: String,
    album: String,
    album_artist: String,
    now: i64,
) -> Option<NewAlbumSource> {
    let rows = downloadable_rows(preview);
    if rows.is_empty() {
        return None;
    }
    let tracks_json = serde_json::to_string(&rows).ok()?;
    Some(NewAlbumSource {
        platform: platform_to_str(preview.platform).to_string(),
        album_url,
        album_key,
        album_artist,
        album,
        cover_url: preview.cover_url.clone(),
        year: preview.year,
        track_urls: rows.iter().map(|r| r.url.clone()).collect(),
        tracks_json: Some(tracks_json),
        fetched_at: now,
    })
}

/// The `album_sources` row for a just-parsed album (D5): the WHOLE track list, not only the selected tracks.
pub fn build_album_source(
    platform: Platform,
    url: &str,
    album: &AlbumDownload,
    skipped: &[SkippedTrack],
    now: i64,
) -> Option<NewAlbumSource> {
    let album_key = normalize_url_key(url)?;
    let preview = build_preview(platform, url, album.clone(), skipped);
    source_from_preview(
        &preview,
        album_key,
        url.trim().to_string(),
        decode_html_entities(&album.title).to_string(),
        decode_html_entities(&album.album_artist).to_string(),
        now,
    )
}

/// The row that links a library album (`album`, `album_artist`: what its songs are tagged with) to the platform
/// album `preview` that the user picked. None when the url has no key or the album has no downloadable track.
pub fn linked_source(
    preview: &AlbumPreview,
    album_url: &str,
    album: &str,
    album_artist: &str,
    now: i64,
) -> Option<NewAlbumSource> {
    let album_key = normalize_url_key(album_url)?;
    source_from_preview(
        preview,
        album_key,
        album_url.trim().to_string(),
        album.to_string(),
        album_artist.to_string(),
        now,
    )
}

/// The stored row updated with a fresh preview. Key, url, library album title and artist stay (the library
/// songs were tagged with them); cover and year are only replaced when the platform still gives one. None when
/// the fresh preview has no downloadable track (a failed parse must not wipe the stored list).
pub fn refreshed_source(row: &AlbumSourceRow, preview: &AlbumPreview, now: i64) -> Option<NewAlbumSource> {
    let mut source = source_from_preview(
        preview,
        row.album_key.clone(),
        row.album_url.clone(),
        row.album.clone(),
        row.album_artist.clone(),
        now,
    )?;
    source.platform = row.platform.clone();
    if source.cover_url.is_none() {
        source.cover_url = row.cover_url.clone();
    }
    if source.year.is_none() {
        source.year = row.year;
    }
    Some(source)
}

/// Downloadable tracks of a source: `tracks_json` when it parses, else the bare urls of `track_urls` (rows
/// written without details; they have no title, so the UI can only count them).
pub fn parse_source_tracks(tracks_json: Option<&str>, track_urls_json: &str) -> Vec<AlbumTrackInfo> {
    let parsed = tracks_json.and_then(|json| serde_json::from_str::<Vec<AlbumTrackInfo>>(json).ok());
    let tracks = match parsed {
        Some(tracks) => tracks,
        None => serde_json::from_str::<Vec<String>>(track_urls_json)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, url)| AlbumTrackInfo {
                index,
                title: String::new(),
                artists: Vec::new(),
                track_number: i32::try_from(index + 1).ok(),
                url,
                available: true,
                note: None,
                duration_ms: None,
            })
            .collect(),
    };
    tracks.into_iter().filter(|t| t.available && !t.url.trim().is_empty()).collect()
}

pub fn row_tracks(row: &AlbumSourceRow) -> Vec<AlbumTrackInfo> {
    parse_source_tracks(row.tracks_json.as_deref(), &row.track_urls)
}

pub fn row_tracks_of_new(source: &NewAlbumSource) -> Vec<AlbumTrackInfo> {
    let urls = serde_json::to_string(&source.track_urls).unwrap_or_else(|_| "[]".to_string());
    parse_source_tracks(source.tracks_json.as_deref(), &urls)
}

/// True when the library has the remote track: by page url, else (songs added from files have no url) by
/// artist and title.
fn owns_track(
    platform: Platform,
    album_artist: &str,
    track: &AlbumTrackInfo,
    siblings: &[AlbumTrackInfo],
    index: &LibraryIndex,
) -> bool {
    if index.contains_track(platform, &track.url) {
        return true;
    }
    let title = decode_html_entities(&track.title);
    if index.contains_numbered(track.track_number, &title) {
        return true;
    }
    let artist = track.artists.first().map_or(album_artist, String::as_str);
    index.contains_by_meta(artist, &title)
        || index.contains_by_meta(album_artist, &title)
        || (!is_variant_of_sibling(track, siblings) && index.contains_by_fuzzy_title(album_artist, &title))
}

/// True when another track of the album has the same title once brackets are ignored but a shorter plain one
/// ("Title (interlude)" next to "Title"): the plain track owns the library song, the variant must not borrow it.
fn is_variant_of_sibling(track: &AlbumTrackInfo, siblings: &[AlbumTrackInfo]) -> bool {
    use crate::library_index::candidates::normalize_text;
    let own = normalize_text(&track.title);
    let own_len = track.title.trim().chars().count();
    siblings.iter().any(|s| {
        s.url != track.url
            && normalize_text(&s.title) == own
            && s.title.trim().chars().count() < own_len
    })
}

/// (owned, total) of the downloadable tracks.
pub fn count_owned(
    platform: Platform,
    album_artist: &str,
    tracks: &[AlbumTrackInfo],
    index: &LibraryIndex,
) -> (usize, usize) {
    let downloadable = tracks.iter().filter(|t| t.available && !t.url.trim().is_empty());
    let (mut owned, mut total) = (0, 0);
    for track in downloadable {
        total += 1;
        if owns_track(platform, album_artist, track, tracks, index) {
            owned += 1;
        }
    }
    (owned, total)
}

/// Downloadable tracks that are not in the library, in album order.
pub fn compute_missing(
    platform: Platform,
    album_artist: &str,
    tracks: &[AlbumTrackInfo],
    index: &LibraryIndex,
) -> Vec<AlbumTrackInfo> {
    tracks
        .iter()
        .filter(|t| t.available && !t.url.trim().is_empty() && !owns_track(platform, album_artist, t, tracks, index))
        .cloned()
        .collect()
}

/// Where a single track (e.g. from another platform) belongs when the library already has the album: the
/// (album, album artist, track number) of the same-titled track in a stored source of the artist's albums.
pub fn find_in_sources(rows: &[AlbumSourceRow], artist: &str, title: &str) -> Option<(String, String, i32)> {
    use crate::library_index::candidates::normalize_text;
    use crate::library_index::status::edit_similarity;
    let wanted = normalize_text(title);
    if wanted.chars().count() < 2 {
        return None;
    }
    let artist = normalize_text(artist.split(',').next().unwrap_or(artist));
    for row in rows.iter().filter(|r| normalize_text(&r.album_artist) == artist) {
        for (i, track) in row_tracks(row).iter().enumerate() {
            let candidate = normalize_text(&decode_html_entities(&track.title));
            if candidate == wanted || edit_similarity(&candidate, &wanted) >= 0.9 {
                let number = track.track_number.unwrap_or(i as i32 + 1);
                return Some((row.album.clone(), row.album_artist.clone(), number));
            }
        }
    }
    None
}

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The source of a library album (D8): exact (album_artist, album) match ignoring case and surrounding
/// whitespace; else, for renamed albums, the source of which most tracks are among `song_refs` (the album's
/// songs). None when nothing matches.
pub fn match_source<'a>(
    rows: &'a [AlbumSourceRow],
    album_artist: &str,
    album: &str,
    song_refs: &[SongSourceRef],
) -> Option<&'a AlbumSourceRow> {
    if let Some(row) = rows
        .iter()
        .find(|r| same_text(&r.album_artist, album_artist) && same_text(&r.album, album))
    {
        return Some(row);
    }

    let index = LibraryIndex::from_rows(song_refs);
    if index.keys.is_empty() {
        return None;
    }
    let mut best: Option<(&AlbumSourceRow, usize)> = None;
    for row in rows {
        let Some(platform) = platform_from_str(&row.platform) else {
            continue;
        };
        let overlap = row_tracks(row)
            .iter()
            .filter(|t| index.contains_track(platform, &t.url))
            .count();
        if overlap > 0 && best.map_or(true, |(_, n)| overlap > n) {
            best = Some((row, overlap));
        }
    }
    best.map(|(row, _)| row)
}

/// A small time-limited map (the refreshed previews are cached for an hour, D8). `now` is a parameter so the
/// expiry is testable without sleeping.
pub struct TtlCache<V: Clone> {
    ttl: Duration,
    entries: HashMap<String, (Instant, V)>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, entries: HashMap::new() }
    }

    pub fn get(&self, key: &str, now: Instant) -> Option<V> {
        let (stored_at, value) = self.entries.get(key)?;
        (now.saturating_duration_since(*stored_at) < self.ttl).then(|| value.clone())
    }

    pub fn put(&mut self, key: String, value: V, now: Instant) {
        let ttl = self.ttl;
        self.entries.retain(|_, (at, _)| now.saturating_duration_since(*at) < ttl);
        self.entries.insert(key, (now, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::TrackDownload;
    use crate::utils::make_canonical_slug;

    fn track(title: &str, url: &str, n: i32, duration: Option<u64>) -> TrackDownload {
        TrackDownload {
            url: url.to_string(),
            title: title.to_string(),
            artists: vec!["Artist".to_string()],
            track_number: Some(n),
            duration_ms: duration,
            ..TrackDownload::default()
        }
    }

    fn album(tracks: Vec<TrackDownload>) -> AlbumDownload {
        AlbumDownload {
            title: "The Album".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            tracks,
            genres: None,
            release_year: Some(2021),
            external_cover_url: Some("https://img/c.jpg".to_string()),
        }
    }

    fn info(index: usize, url: &str, available: bool) -> AlbumTrackInfo {
        AlbumTrackInfo {
            index,
            title: format!("t{index}"),
            artists: vec![],
            track_number: Some(index as i32 + 1),
            url: url.to_string(),
            available,
            note: None,
            duration_ms: None,
        }
    }

    fn song(source: &str, url: Option<&str>, item_id: Option<&str>) -> SongSourceRef {
        SongSourceRef {
            source: source.to_string(),
            source_url: url.map(str::to_string),
            source_item_id: item_id.map(str::to_string),
        }
    }

    fn row(id: i64, artist: &str, album: &str, platform: &str, urls: &[&str]) -> AlbumSourceRow {
        let tracks: Vec<AlbumTrackInfo> = urls.iter().enumerate().map(|(i, u)| info(i, u, true)).collect();
        AlbumSourceRow {
            id,
            platform: platform.to_string(),
            album_url: format!("https://x/{id}"),
            album_key: format!("x/{id}"),
            album_artist: artist.to_string(),
            album: album.to_string(),
            cover_url: Some("https://img/old.jpg".to_string()),
            year: Some(2000),
            track_urls: serde_json::to_string(&urls).unwrap(),
            tracks_json: Some(serde_json::to_string(&tracks).unwrap()),
            fetched_at: 1,
        }
    }

    // ---- build_album_source

    #[test]
    fn build_album_source_stores_the_whole_album() {
        let src = build_album_source(
            Platform::Bandcamp,
            "https://Artist.bandcamp.com/album/the-album/?from=embed",
            &album(vec![
                track("One", "https://a.bandcamp.com/track/one", 1, Some(61_000)),
                track("Two", "https://a.bandcamp.com/track/two", 2, None),
            ]),
            &[],
            1234,
        )
        .expect("source");
        assert_eq!(src.platform, "bandcamp");
        assert_eq!(src.album_key, "artist.bandcamp.com/album/the-album");
        assert_eq!(src.album_url, "https://Artist.bandcamp.com/album/the-album/?from=embed");
        assert_eq!(src.album, "The Album");
        assert_eq!(src.album_artist, "Artist");
        assert_eq!(src.year, Some(2021));
        assert_eq!(src.cover_url.as_deref(), Some("https://img/c.jpg"));
        assert_eq!(src.fetched_at, 1234);
        assert_eq!(src.track_urls, vec!["https://a.bandcamp.com/track/one", "https://a.bandcamp.com/track/two"]);
        let stored: Vec<AlbumTrackInfo> = serde_json::from_str(src.tracks_json.as_deref().unwrap()).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].duration_ms, Some(61_000));
        assert_eq!(stored[1].duration_ms, None);
    }

    #[test]
    fn build_album_source_drops_unavailable_tracks_but_keeps_indexes() {
        let skipped = vec![SkippedTrack { position: 2, title: "Go+".to_string(), reason: "preview".to_string() }];
        let src = build_album_source(
            Platform::Soundcloud,
            "https://soundcloud.com/a/sets/b",
            &album(vec![track("One", "https://sc/1", 1, None), track("Three", "https://sc/3", 3, None)]),
            &skipped,
            1,
        )
        .unwrap();
        assert_eq!(src.track_urls, vec!["https://sc/1", "https://sc/3"]);
        let stored: Vec<AlbumTrackInfo> = serde_json::from_str(src.tracks_json.as_deref().unwrap()).unwrap();
        assert_eq!(stored.iter().map(|t| t.index).collect::<Vec<_>>(), vec![0, 2], "album position survives");
        assert!(stored.iter().all(|t| t.available));
    }

    #[test]
    fn build_album_source_decodes_html_entities_like_the_post_processor() {
        let mut a = album(vec![track("Rock &amp; Roll", "https://u/1", 1, None)]);
        a.title = "Fish &amp; Chips".to_string();
        a.album_artist = "Simon &amp; Co".to_string();
        let src = build_album_source(Platform::Bandcamp, "https://a.bandcamp.com/album/x", &a, &[], 1).unwrap();
        assert_eq!(src.album, "Fish & Chips");
        assert_eq!(src.album_artist, "Simon & Co");
        let stored: Vec<AlbumTrackInfo> = serde_json::from_str(src.tracks_json.as_deref().unwrap()).unwrap();
        assert_eq!(stored[0].title, "Rock & Roll");
    }

    #[test]
    fn build_album_source_is_none_without_key_or_tracks() {
        let a = album(vec![track("One", "https://u/1", 1, None)]);
        assert!(build_album_source(Platform::Bandcamp, "", &a, &[], 1).is_none());
        assert!(build_album_source(Platform::Bandcamp, "garbage", &a, &[], 1).is_none());
        assert!(build_album_source(Platform::Bandcamp, "https://a.bandcamp.com/album/x", &album(vec![]), &[], 1).is_none());
    }

    // ---- refreshed_source

    #[test]
    fn refreshed_source_keeps_identity_and_updates_the_track_list() {
        let old = row(7, "Artist", "Library Title", "bandcamp", &["https://u/1"]);
        let preview = build_preview(
            Platform::Bandcamp,
            "https://ignored",
            album(vec![track("One", "https://u/1", 1, None), track("Two", "https://u/2", 2, None)]),
            &[],
        );
        let fresh = refreshed_source(&old, &preview, 55).unwrap();
        assert_eq!(fresh.album_key, "x/7");
        assert_eq!(fresh.album_url, "https://x/7");
        assert_eq!(fresh.album, "Library Title", "the library title is kept");
        assert_eq!(fresh.album_artist, "Artist");
        assert_eq!(fresh.track_urls, vec!["https://u/1", "https://u/2"]);
        assert_eq!(fresh.fetched_at, 55);
        assert_eq!(row_tracks_of_new(&fresh).len(), 2);
    }

    #[test]
    fn refreshed_source_keeps_old_cover_and_year_when_the_platform_gives_none() {
        let old = row(1, "A", "B", "bandcamp", &["https://u/1"]);
        let mut a = album(vec![track("One", "https://u/1", 1, None)]);
        a.external_cover_url = None;
        a.release_year = None;
        let preview = build_preview(Platform::Bandcamp, "u", a, &[]);
        let fresh = refreshed_source(&old, &preview, 1).unwrap();
        assert_eq!(fresh.cover_url.as_deref(), Some("https://img/old.jpg"));
        assert_eq!(fresh.year, Some(2000));
    }

    #[test]
    fn refreshed_source_refuses_an_empty_preview() {
        let old = row(1, "A", "B", "bandcamp", &["https://u/1"]);
        let preview = build_preview(Platform::Bandcamp, "u", album(vec![]), &[]);
        assert!(refreshed_source(&old, &preview, 1).is_none());
    }

    // ---- parse_source_tracks

    #[test]
    fn parse_source_tracks_prefers_the_details() {
        let json = serde_json::to_string(&vec![info(0, "https://u/1", true), info(1, "https://u/2", false)]).unwrap();
        let tracks = parse_source_tracks(Some(&json), r#"["https://other"]"#);
        assert_eq!(tracks.len(), 1, "unavailable rows are dropped");
        assert_eq!(tracks[0].title, "t0");
    }

    #[test]
    fn parse_source_tracks_falls_back_to_the_urls() {
        let tracks = parse_source_tracks(None, r#"["https://u/1","https://u/2"]"#);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].url, "https://u/2");
        assert_eq!(tracks[1].track_number, Some(2));
        assert!(tracks[1].available);
        // a corrupt details column falls back too
        assert_eq!(parse_source_tracks(Some("not json"), r#"["https://u/1"]"#).len(), 1);
    }

    #[test]
    fn parse_source_tracks_survives_garbage() {
        assert!(parse_source_tracks(None, "garbage").is_empty());
        assert!(parse_source_tracks(Some("[]"), "[]").is_empty());
    }

    // ---- count_owned / compute_missing

    fn index_of(refs: &[SongSourceRef]) -> LibraryIndex {
        LibraryIndex::from_rows(refs)
    }

    #[test]
    fn compute_missing_none_owned() {
        let tracks = vec![info(0, "https://a.bandcamp.com/track/1", true), info(1, "https://a.bandcamp.com/track/2", true)];
        let missing = compute_missing(Platform::Bandcamp, "A", &tracks, &index_of(&[]));
        assert_eq!(missing.len(), 2);
        assert_eq!(count_owned(Platform::Bandcamp, "A", &tracks, &index_of(&[])), (0, 2));
    }

    #[test]
    fn compute_missing_partial_keeps_album_order() {
        let tracks = vec![
            info(0, "https://a.bandcamp.com/track/1", true),
            info(1, "https://a.bandcamp.com/track/2", true),
            info(2, "https://a.bandcamp.com/track/3", true),
        ];
        let index = index_of(&[song("bandcamp", Some("https://a.bandcamp.com/track/2/"), None)]);
        let missing = compute_missing(Platform::Bandcamp, "A", &tracks, &index);
        assert_eq!(missing.iter().map(|t| t.index).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(count_owned(Platform::Bandcamp, "A", &tracks, &index), (1, 3));
    }

    #[test]
    fn compute_missing_all_owned() {
        let tracks = vec![info(0, "https://a.bandcamp.com/track/1", true)];
        let index = index_of(&[song("bandcamp", Some("https://a.bandcamp.com/track/1"), None)]);
        assert!(compute_missing(Platform::Bandcamp, "A", &tracks, &index).is_empty());
        assert_eq!(count_owned(Platform::Bandcamp, "A", &tracks, &index), (1, 1));
    }

    #[test]
    fn compute_missing_skips_unavailable_and_urlless_tracks() {
        let tracks = vec![info(0, "https://u/1", false), info(1, "", true), info(2, "https://u/3", true)];
        let missing = compute_missing(Platform::Soundcloud, "A", &tracks, &index_of(&[]));
        assert_eq!(missing.iter().map(|t| t.index).collect::<Vec<_>>(), vec![2]);
        assert_eq!(count_owned(Platform::Soundcloud, "A", &tracks, &index_of(&[])), (0, 1));
    }

    #[test]
    fn compute_missing_uses_the_youtube_item_id_fallback() {
        let tracks = vec![
            info(0, "https://music.youtube.com/watch?v=aaa", true),
            info(1, "https://music.youtube.com/watch?v=bbb", true),
        ];
        // owned song has a different url form but the same item id
        let index = index_of(&[song("youtube", Some("https://example.com/other"), Some("aaa"))]);
        let missing = compute_missing(Platform::Youtube, "A", &tracks, &index);
        assert_eq!(missing.iter().map(|t| t.index).collect::<Vec<_>>(), vec![1]);
    }

    fn numbered_track(number: i32, title: &str, url: &str) -> AlbumTrackInfo {
        AlbumTrackInfo {
            index: (number - 1) as usize,
            title: title.to_string(),
            artists: vec![],
            track_number: Some(number),
            url: url.to_string(),
            available: true,
            note: None,
            duration_ms: None,
        }
    }

    /// Titles of the library album, as stored: the song at position i+1 has this (track number, title).
    const LIBRARY_TITLES: [&str; 10] = [
        "Digeridoo",
        "Bonfire",
        "Fell Down",
        "Hall of Mirrors",
        "Sea Mist",
        "Pipe Dream (Live In Cornwall) (Cr7e Version)",
        "Old Stone",
        "Crossing",
        "Blue Hour",
        "Last Light",
    ];

    /// Titles of the same album on the platform: remaster / version suffixes in brackets, or a case change.
    const PLATFORM_TITLES: [&str; 10] = [
        "Digeridoo (2022 Remaster)",
        "Bonfire (Live)",
        "Fell Down [Remastered]",
        "Hall of Mirrors (Mono)",
        "Sea Mist (2022 Remaster)",
        "Pipe Dream (Cr7E Version, Live In Cornwall)",
        "Old Stone (Demo)",
        "Crossing (Remix)",
        "Blue Hour (Radio Edit)",
        "LAST LIGHT (2022 Remaster)",
    ];

    fn digeridoo_index() -> LibraryIndex {
        LibraryIndex::default().with_numbered_titles(
            LIBRARY_TITLES.iter().enumerate().map(|(i, t)| (i as i32 + 1, t.to_string())),
        )
    }

    fn digeridoo_tracks() -> Vec<AlbumTrackInfo> {
        PLATFORM_TITLES
            .iter()
            .enumerate()
            .map(|(i, t)| numbered_track(i as i32 + 1, t, &format!("https://yt.test/watch?v=vid{}", i + 1)))
            .collect()
    }

    #[test]
    fn remastered_and_versioned_tracks_are_owned_by_track_number_and_title() {
        let tracks = digeridoo_tracks();
        let index = digeridoo_index();
        assert_eq!(count_owned(Platform::Youtube, "Artist", &tracks, &index), (10, 10));
        assert!(compute_missing(Platform::Youtube, "Artist", &tracks, &index).is_empty());
    }

    #[test]
    fn a_track_whose_number_matches_but_whose_title_differs_stays_missing() {
        let tracks = vec![numbered_track(1, "Another Song (2022 Remaster)", "https://yt.test/watch?v=x")];
        let index = LibraryIndex::default().with_numbered_titles([(1, "Digeridoo".to_string())]);
        assert_eq!(compute_missing(Platform::Youtube, "Artist", &tracks, &index).len(), 1);
        assert_eq!(count_owned(Platform::Youtube, "Artist", &tracks, &index), (0, 1));
    }

    #[test]
    fn a_track_without_a_track_number_is_not_matched_by_title_alone() {
        let mut track = numbered_track(1, "Digeridoo (2022 Remaster)", "https://yt.test/watch?v=x");
        track.track_number = None;
        let tracks = vec![track];
        let index = LibraryIndex::default().with_numbered_titles([(1, "Digeridoo".to_string())]);
        assert_eq!(compute_missing(Platform::Youtube, "Artist", &tracks, &index).len(), 1);
        assert_eq!(count_owned(Platform::Youtube, "Artist", &tracks, &index), (0, 1));
    }

    fn titled(title: &str, url: &str, artists: &[&str]) -> AlbumTrackInfo {
        AlbumTrackInfo {
            index: 0,
            title: title.to_string(),
            artists: artists.iter().map(|a| a.to_string()).collect(),
            track_number: Some(1),
            url: url.to_string(),
            available: true,
            note: None,
            duration_ms: None,
        }
    }

    fn slug_index(artist: &str, title: &str) -> LibraryIndex {
        LibraryIndex::default().with_slugs([make_canonical_slug(artist, title)])
    }

    #[test]
    fn slug_match_makes_a_track_owned_when_the_url_is_unknown() {
        let tracks = vec![titled("Track One", "https://a.bandcamp.com/track/one", &["Artist"])];
        let index = slug_index("Artist", "Track One");
        assert!(compute_missing(Platform::Bandcamp, "Artist", &tracks, &index).is_empty());
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
    }

    #[test]
    fn slug_match_leaves_a_track_with_another_title_missing() {
        let tracks = vec![titled("Track Two", "https://a.bandcamp.com/track/two", &["Artist"])];
        let index = slug_index("Artist", "Track One");
        assert_eq!(compute_missing(Platform::Bandcamp, "Artist", &tracks, &index).len(), 1);
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (0, 1));
    }

    #[test]
    fn slug_match_decodes_html_entities_in_the_title() {
        let tracks = vec![titled("Tom &amp; Jerry", "https://u/tj", &["Artist"])];
        let index = slug_index("Artist", "Tom & Jerry");
        assert!(compute_missing(Platform::Bandcamp, "Artist", &tracks, &index).is_empty());
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
    }

    #[test]
    fn slug_match_uses_the_track_artist_when_present() {
        // credited to "Other" and the library has it under "Other", the album artist is not involved
        let tracks = vec![titled("Track One", "https://u/1", &["Other"])];
        let index = slug_index("Other", "Track One");
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
    }

    #[test]
    fn slug_match_uses_the_album_artist_when_the_track_has_no_artist() {
        let tracks = vec![titled("Track One", "https://u/1", &[])];
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &slug_index("Artist", "Track One")), (1, 1));
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &slug_index("Someone", "Track One")), (0, 1));
    }

    #[test]
    fn slug_match_also_tries_the_album_artist_when_the_track_credits_another_artist() {
        // the library files a track under the album artist, even when the track credits a guest
        let tracks = vec![titled("Track One", "https://u/1", &["Other"])];
        let index = slug_index("Artist", "Track One");
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
    }

    #[test]
    fn fuzzy_title_makes_a_track_owned_when_the_library_title_has_a_guest_suffix() {
        let tracks = vec![titled("Title", "https://unknown.test/title", &["Artist"])];
        let index = LibraryIndex::default().with_titles([("Artist".into(), "Title(+ Guest)".into())]);
        assert!(compute_missing(Platform::Bandcamp, "Artist", &tracks, &index).is_empty());
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
    }

    #[test]
    fn fuzzy_title_ownership_uses_the_album_artist_when_the_track_has_no_artist() {
        let tracks = vec![titled("Title", "https://unknown.test/title", &[])];
        let index = LibraryIndex::default().with_titles([("Artist".into(), "Title(+ Guest)".into())]);
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 1));
        assert_eq!(count_owned(Platform::Bandcamp, "Someone", &tracks, &index), (0, 1));
    }

    #[test]
    fn fuzzy_title_does_not_lend_the_plain_song_to_its_bracketed_variant() {
        // "Взаперти" is in the library; "Взаперти (интерлюдия)" is a different track of the same album
        let tracks = vec![
            titled("Взаперти (интерлюдия)", "https://u/4", &["Artist"]),
            titled("Взаперти", "https://u/5", &["Artist"]),
        ];
        let index = LibraryIndex::default().with_titles([("Artist".into(), "Взаперти".into())]);
        let missing = compute_missing(Platform::Bandcamp, "Artist", &tracks, &index);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].url, "https://u/4");
        assert_eq!(count_owned(Platform::Bandcamp, "Artist", &tracks, &index), (1, 2));
    }

    #[test]
    fn find_in_sources_gives_the_album_and_number_of_a_same_titled_track() {
        let rows = vec![row(1, "Artist", "The Album", "bandcamp", &["https://u/1", "https://u/2"])];
        // info() names the tracks t0, t1, ...
        let found = find_in_sources(&rows, "Artist, Guest", "T1").expect("found");
        assert_eq!(found, ("The Album".to_string(), "Artist".to_string(), 2));
        assert!(find_in_sources(&rows, "Someone", "t1").is_none());
        assert!(find_in_sources(&rows, "Artist", "other title").is_none());
    }

    // ---- match_source

    #[test]
    fn match_source_exact() {
        let rows = vec![row(1, "Artist", "Other", "bandcamp", &["u1"]), row(2, "Artist", "Album", "bandcamp", &["u2"])];
        assert_eq!(match_source(&rows, "Artist", "Album", &[]).map(|r| r.id), Some(2));
    }

    #[test]
    fn match_source_ignores_case_and_whitespace() {
        let rows = vec![row(1, "The Artist", "Great Album", "bandcamp", &["u1"])];
        assert_eq!(match_source(&rows, "  the artist ", "GREAT album", &[]).map(|r| r.id), Some(1));
    }

    #[test]
    fn match_source_falls_back_to_track_overlap_after_a_rename() {
        let rows = vec![
            row(1, "Artist", "Original", "bandcamp", &["https://a.bandcamp.com/track/1", "https://a.bandcamp.com/track/2"]),
            row(2, "Artist", "Unrelated", "bandcamp", &["https://a.bandcamp.com/track/9"]),
        ];
        let refs = vec![song("bandcamp", Some("https://a.bandcamp.com/track/2"), None)];
        assert_eq!(match_source(&rows, "Artist", "Renamed", &refs).map(|r| r.id), Some(1));
    }

    #[test]
    fn match_source_overlap_picks_the_best_candidate() {
        let rows = vec![
            row(1, "A", "X", "bandcamp", &["https://u.bandcamp.com/t/1"]),
            row(2, "A", "Y", "bandcamp", &["https://u.bandcamp.com/t/1", "https://u.bandcamp.com/t/2"]),
        ];
        let refs = vec![
            song("bandcamp", Some("https://u.bandcamp.com/t/1"), None),
            song("bandcamp", Some("https://u.bandcamp.com/t/2"), None),
        ];
        assert_eq!(match_source(&rows, "A", "Z", &refs).map(|r| r.id), Some(2));
    }

    #[test]
    fn match_source_overlap_works_for_youtube_item_ids() {
        let rows = vec![row(1, "A", "X", "youtube", &["https://music.youtube.com/watch?v=aaa"])];
        let refs = vec![song("youtube", None, Some("aaa"))];
        assert_eq!(match_source(&rows, "A", "Renamed", &refs).map(|r| r.id), Some(1));
    }

    #[test]
    fn match_source_no_match() {
        let rows = vec![row(1, "Artist", "Album", "bandcamp", &["https://a.bandcamp.com/track/1"])];
        assert!(match_source(&rows, "Artist", "Nope", &[]).is_none());
        let refs = vec![song("bandcamp", Some("https://a.bandcamp.com/track/other"), None)];
        assert!(match_source(&rows, "Artist", "Nope", &refs).is_none());
        assert!(match_source(&[], "Artist", "Album", &refs).is_none());
    }

    #[test]
    fn match_source_exact_match_wins_over_overlap() {
        let rows = vec![
            row(1, "A", "Other", "bandcamp", &["https://u.bandcamp.com/t/1"]),
            row(2, "A", "Mine", "bandcamp", &["https://u.bandcamp.com/t/7"]),
        ];
        let refs = vec![song("bandcamp", Some("https://u.bandcamp.com/t/1"), None)];
        assert_eq!(match_source(&rows, "A", "Mine", &refs).map(|r| r.id), Some(2));
    }

    // ---- TtlCache

    #[test]
    fn ttl_cache_returns_fresh_and_drops_expired_entries() {
        let start = Instant::now();
        let mut cache = TtlCache::new(Duration::from_secs(3600));
        cache.put("k".to_string(), 5, start);
        assert_eq!(cache.get("k", start + Duration::from_secs(10)), Some(5));
        assert_eq!(cache.get("k", start + Duration::from_secs(3599)), Some(5));
        assert_eq!(cache.get("k", start + Duration::from_secs(3600)), None);
        assert_eq!(cache.get("other", start), None);
    }

    #[test]
    fn ttl_cache_put_replaces_and_prunes() {
        let start = Instant::now();
        let mut cache = TtlCache::new(Duration::from_secs(60));
        cache.put("old".to_string(), 1, start);
        cache.put("k".to_string(), 2, start + Duration::from_secs(100));
        cache.put("k".to_string(), 3, start + Duration::from_secs(110));
        assert_eq!(cache.get("k", start + Duration::from_secs(111)), Some(3));
        assert_eq!(cache.get("old", start + Duration::from_secs(1)), None, "pruned on the later put");
    }
}
