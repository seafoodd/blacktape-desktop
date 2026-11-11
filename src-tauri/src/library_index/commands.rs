//! Tauri commands of the library index (registered in lib.rs; names and shapes are FINAL). Owner: part A.
//! Never hold the database mutex across a network call (`get_missing_tracks` with refresh): every command reads
//! its rows under the lock, drops the guard and computes on plain values (the pure helpers below and in
//! `sources.rs` are what the tests cover).

use crate::db::album_sources::{AlbumSongSourceRef, AlbumSourceRow, SongMeta, SongSourceRef};
use crate::db::db::Database;
use crate::download::album_preview::{album_preview, AlbumPreview, AlbumTrackInfo};
use crate::library_index::normalize::normalize_url_key;
use crate::library_index::sources::{
    compute_missing, count_owned, match_source, refreshed_source, row_tracks, row_tracks_of_new, TtlCache,
};
use crate::library_index::status::{
    album_state, platform_from_str, slugs_contain, AlbumStatus, DownloadState, LibraryIndex, RemoteRef,
};
use crate::types::Platform;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{command, State};

/// Refreshed album previews are kept for an hour so a user toggling "show missing" does not hit the platform
/// every time.
const PREVIEW_TTL: Duration = Duration::from_secs(60 * 60);

fn preview_cache() -> &'static Mutex<TtlCache<AlbumPreview>> {
    static CACHE: OnceLock<Mutex<TtlCache<AlbumPreview>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TtlCache::new(PREVIEW_TTL)))
}

fn db_error(e: sqlx::Error) -> String {
    format!("Library database error: {e}")
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Library state of every requested track url (`downloaded` or `none`); keyed by the url exactly as sent. A track
/// counts by its url, else by artist + title: among the songs of its album when the item names one (compared
/// without brackets, punctuation and case), else among all songs. Pure.
pub fn track_states(items: &[RemoteRef], index: &LibraryIndex, meta: &[SongMeta]) -> HashMap<String, DownloadState> {
    use crate::library_index::candidates::normalize_text;
    let mut slugs_by_album: HashMap<String, HashSet<String>> = HashMap::new();
    for (_, album, _, slug, _) in meta {
        slugs_by_album.entry(normalize_text(album)).or_default().insert(slug.clone());
    }
    items
        .iter()
        .map(|item| {
            let album = item.album.as_deref().map(normalize_text).filter(|a| !a.is_empty());
            let by_meta = match (item.artist.as_deref(), item.title.as_deref(), album) {
                (Some(artist), Some(title), Some(album)) => slugs_by_album
                    .get(&album)
                    .is_some_and(|slugs| slugs_contain(slugs, artist, title)),
                (Some(artist), Some(title), None) => index.contains_by_meta(artist, title),
                _ => false,
            };
            let state = if index.contains_track(item.platform, &item.url) || by_meta {
                DownloadState::Downloaded
            } else {
                DownloadState::None
            };
            (item.url.clone(), state)
        })
        .collect()
}

/// Status of one stored album source against the library. Pure.
fn source_status(row: &AlbumSourceRow, fallback: Platform, index: &LibraryIndex) -> AlbumStatus {
    let platform = platform_from_str(&row.platform).unwrap_or(fallback);
    let (owned, total) = count_owned(platform, &row.album_artist, &row_tracks(row), index);
    AlbumStatus { state: album_state(owned, total), owned, total }
}

/// `base` (url keys of the whole library) plus the names of the songs of one library album only. A song with the
/// same artist and title in ANOTHER album (a live or compilation version) must not make this album's track
/// "owned": url keys are exact, names are not. Pure.
pub fn scoped_index(base: &LibraryIndex, meta: &[SongMeta], album_artist: &str, album: &str) -> LibraryIndex {
    let same = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
    let own = || meta.iter().filter(|(artist, alb, ..)| same(artist, album_artist) && same(alb, album));
    base.clone()
        .with_slugs(own().map(|(_, _, _, slug, _)| slug.clone()))
        .with_titles(own().map(|(artist, _, title, ..)| (artist.clone(), title.clone())))
        .with_numbered_titles(own().filter_map(|(_, _, title, _, number)| number.map(|n| (n, title.clone()))))
}

/// Status of every requested album url from the stored sources (`sources` is keyed by `album_key`); albums
/// without a source are `none` with zero counts. Pure.
pub fn album_statuses(
    items: &[RemoteRef],
    sources: &HashMap<String, AlbumSourceRow>,
    index: &LibraryIndex,
) -> HashMap<String, AlbumStatus> {
    items
        .iter()
        .map(|item| {
            let status = normalize_url_key(&item.url)
                .and_then(|key| sources.get(&key))
                .map(|row| source_status(row, item.platform, index))
                .unwrap_or(AlbumStatus { state: DownloadState::None, owned: 0, total: 0 });
            (item.url.clone(), status)
        })
        .collect()
}

/// Library state of remote TRACK urls. The answer is keyed by the url exactly as sent.
#[command]
pub async fn get_download_status(
    db: State<'_, tokio::sync::Mutex<Database>>,
    items: Vec<RemoteRef>,
) -> Result<HashMap<String, DownloadState>, String> {
    let (refs, slugs, meta) = {
        let guard = db.lock().await;
        let refs = guard.list_song_source_refs().await.map_err(db_error)?;
        let slugs = guard.list_track_slugs().await.map_err(db_error)?;
        // only needed to scope the name match of tracks that name their album
        let meta = if items.iter().any(|i| i.album.is_some()) {
            guard.list_song_meta().await.map_err(db_error)?
        } else {
            Vec::new()
        };
        (refs, slugs, meta)
    };
    Ok(track_states(&items, &LibraryIndex::from_rows(&refs).with_slugs(slugs), &meta))
}

/// Where the library keeps an album that was downloaded from a platform page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryAlbumRef {
    pub album_artist: String,
    pub album: String,
}

/// The library album a remote album url was downloaded into (`album_sources`), None when the app never downloaded
/// it. Lets the album preview open the album's page ("Go to album").
#[command]
pub async fn get_library_album(
    db: State<'_, tokio::sync::Mutex<Database>>,
    url: String,
) -> Result<Option<LibraryAlbumRef>, String> {
    let Some(key) = normalize_url_key(&url) else {
        return Ok(None);
    };
    let guard = db.lock().await;
    let row = guard.get_album_source_by_key(&key).await.map_err(db_error)?;
    Ok(row.map(|r| LibraryAlbumRef { album_artist: r.album_artist, album: r.album }))
}

/// The stored source (of one artist's albums) whose library album has this title once text in brackets,
/// punctuation and case are ignored: "Syro" finds the library's "Syro" and "Selected Ambient Works Volume II"
/// finds "Selected Ambient Works Volume II (Expanded Edition)". Pure.
pub fn source_by_title<'a>(rows: &'a [AlbumSourceRow], title: &str) -> Option<&'a AlbumSourceRow> {
    use crate::library_index::candidates::normalize_text;
    let wanted = normalize_text(title);
    if wanted.is_empty() {
        return None;
    }
    rows.iter().find(|r| normalize_text(&r.album) == wanted)
}

/// Library state of remote ALBUM urls, from `album_sources` (albums the app never downloaded are `none`). An album
/// url without a source of its own falls back to the library album of the same artist and title.
#[command]
pub async fn get_album_status(
    db: State<'_, tokio::sync::Mutex<Database>>,
    items: Vec<RemoteRef>,
) -> Result<HashMap<String, AlbumStatus>, String> {
    let mut keys: Vec<String> = items.iter().filter_map(|i| normalize_url_key(&i.url)).collect();
    keys.sort();
    keys.dedup();

    let (sources, refs, meta) = {
        let guard = db.lock().await;
        let mut sources = HashMap::new();
        for key in keys {
            if let Some(row) = guard.get_album_source_by_key(&key).await.map_err(db_error)? {
                sources.insert(key, row);
            }
        }
        // An album the library got from ANOTHER platform page (the same album on YouTube and Bandcamp): matched by
        // artist + title, it answers with the status of that library album.
        let mut by_artist: HashMap<String, Vec<AlbumSourceRow>> = HashMap::new();
        for item in &items {
            let Some(key) = normalize_url_key(&item.url) else { continue };
            let (Some(artist), Some(title)) = (item.artist.as_deref(), item.title.as_deref()) else { continue };
            if sources.contains_key(&key) {
                continue;
            }
            let artist_key = artist.trim().to_lowercase();
            if !by_artist.contains_key(&artist_key) {
                let rows = guard.list_album_sources_for_artist(artist).await.map_err(db_error)?;
                by_artist.insert(artist_key.clone(), rows);
            }
            if let Some(row) = source_by_title(&by_artist[&artist_key], title) {
                sources.insert(key, row.clone());
            }
        }
        // Without any known album there is nothing to compare, skip the library scan.
        let (refs, meta) = if sources.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            (
                guard.list_song_source_refs().await.map_err(db_error)?,
                guard.list_song_meta().await.map_err(db_error)?,
            )
        };
        (sources, refs, meta)
    };
    let base = LibraryIndex::from_rows(&refs);
    // every album is compared with the songs of its own library album
    let mut out = HashMap::new();
    for item in &items {
        let index = match normalize_url_key(&item.url).and_then(|key| sources.get(&key)) {
            Some(row) => scoped_index(&base, &meta, &row.album_artist, &row.album),
            None => base.clone(),
        };
        out.extend(album_statuses(std::slice::from_ref(item), &sources, &index));
    }
    Ok(out)
}

/// One library album of an artist that has a known source (db only, no network).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryAlbumSource {
    /// Title of the LIBRARY album (`songs.album`), the key the library page uses.
    pub album: String,
    pub platform: Platform,
    pub source_url: String,
    pub cover_url: Option<String>,
    /// Tracks of the whole remote album / of them in the library / not in the library.
    pub total: usize,
    pub owned: usize,
    pub missing: usize,
}

/// Groups the songs of an artist by library album (case-insensitive, sorted by title) and pairs every album
/// with its source. Albums without a source (or with a source without tracks) are left out. Pure.
pub fn artist_album_sources(
    album_artist: &str,
    sources: &[AlbumSourceRow],
    songs: &[AlbumSongSourceRef],
    index: &LibraryIndex,
) -> Vec<LibraryAlbumSource> {
    if sources.is_empty() {
        return Vec::new();
    }
    // lowercase title -> (title as the library spells it, refs of its songs)
    let mut albums: BTreeMap<String, (String, Vec<SongSourceRef>)> = BTreeMap::new();
    for song in songs {
        let title = song.album.trim();
        if title.is_empty() {
            continue;
        }
        let entry = albums
            .entry(title.to_lowercase())
            .or_insert_with(|| (song.album.clone(), Vec::new()));
        entry.1.push(SongSourceRef {
            source: song.source.clone(),
            source_url: song.source_url.clone(),
            source_item_id: song.source_item_id.clone(),
        });
    }

    albums
        .into_values()
        .filter_map(|(title, refs)| {
            let row = match_source(sources, album_artist, &title, &refs)?;
            let platform = platform_from_str(&row.platform)?;
            let (owned, total) = count_owned(platform, &row.album_artist, &row_tracks(row), index);
            (total > 0).then(|| LibraryAlbumSource {
                album: title,
                platform,
                source_url: row.album_url.clone(),
                cover_url: row.cover_url.clone(),
                total,
                owned,
                missing: total - owned,
            })
        })
        .collect()
}

/// Sources of the albums of `album_artist` (one call per opened artist page). Albums without a source are
/// simply absent from the list.
#[command]
pub async fn get_artist_album_sources(
    db: State<'_, tokio::sync::Mutex<Database>>,
    album_artist: String,
) -> Result<Vec<LibraryAlbumSource>, String> {
    let (sources, songs, refs, meta) = {
        let guard = db.lock().await;
        let sources = guard.list_album_sources_for_artist(&album_artist).await.map_err(db_error)?;
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let songs = guard.list_artist_song_source_refs(&album_artist).await.map_err(db_error)?;
        let refs = guard.list_song_source_refs().await.map_err(db_error)?;
        let meta = guard.list_song_meta().await.map_err(db_error)?;
        (sources, songs, refs, meta)
    };
    let base = LibraryIndex::from_rows(&refs);
    // one album at a time, each with the names of its own songs only
    let mut albums: Vec<String> = songs.iter().map(|s| s.album.clone()).collect();
    albums.sort_by_key(|a| a.to_lowercase());
    albums.dedup_by_key(|a| a.to_lowercase());
    let mut out = Vec::new();
    for album in albums {
        let of_album: Vec<AlbumSongSourceRef> =
            songs.iter().filter(|s| s.album.to_lowercase() == album.to_lowercase()).cloned().collect();
        let index = scoped_index(&base, &meta, &album_artist, &album);
        out.extend(artist_album_sources(&album_artist, &sources, &of_album, &index));
    }
    out.sort_by_key(|a| a.album.to_lowercase());
    Ok(out)
}

/// The tracks of a library album that are not in the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MissingTracks {
    pub platform: Platform,
    pub source_url: String,
    pub album_title: String,
    pub album_artist: String,
    pub cover_url: Option<String>,
    /// Missing, downloadable tracks in album order (same shape as the album preview rows).
    pub tracks: Vec<AlbumTrackInfo>,
}

/// The answer for a source and its (stored or refreshed) track list. None for a platform we cannot download
/// from. Pure.
pub fn build_missing(
    row: &AlbumSourceRow,
    tracks: &[AlbumTrackInfo],
    cover_url: Option<String>,
    index: &LibraryIndex,
) -> Option<MissingTracks> {
    let platform = platform_from_str(&row.platform).filter(|p| *p != Platform::Local)?;
    Some(MissingTracks {
        platform,
        source_url: row.album_url.clone(),
        album_title: row.album.clone(),
        album_artist: row.album_artist.clone(),
        cover_url,
        tracks: compute_missing(platform, &row.album_artist, tracks, index),
    })
}

/// Preview of the album from the platform, cached for an hour. The cache lock is never held across the fetch.
async fn cached_preview(platform: Platform, url: &str) -> Result<AlbumPreview, String> {
    let key = normalize_url_key(url).unwrap_or_else(|| url.to_string());
    let hit = preview_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key, Instant::now());
    if let Some(preview) = hit {
        return Ok(preview);
    }
    let preview = album_preview(platform, url).await?;
    preview_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .put(key, preview.clone(), Instant::now());
    Ok(preview)
}

/// Links the library album (`album_artist`, `album`) to the platform album at `url` (picked from
/// `find_album_candidates`): its whole track list is stored as the album's source.
#[command]
pub async fn link_album_source(
    db: State<'_, tokio::sync::Mutex<Database>>,
    album_artist: String,
    album: String,
    platform: Platform,
    url: String,
) -> Result<(), String> {
    // network: no lock held here
    let preview = cached_preview(platform, &url).await?;
    let source = crate::library_index::sources::linked_source(&preview, &url, &album, &album_artist, unix_now())
        .ok_or_else(|| "The platform returned no downloadable tracks for this album.".to_string())?;
    let guard = db.lock().await;
    // one platform album per library album: the previous link (if any) goes
    guard.delete_album_sources_for(&album_artist, &album).await.map_err(db_error)?;
    guard.upsert_album_source(&source).await.map_err(db_error)
}

/// Removes the link of the library album (`album_artist`, `album`) to its platform album.
#[command]
pub async fn unlink_album_source(
    db: State<'_, tokio::sync::Mutex<Database>>,
    album_artist: String,
    album: String,
) -> Result<(), String> {
    let guard = db.lock().await;
    guard.delete_album_sources_for(&album_artist, &album).await.map_err(db_error)?;
    Ok(())
}

/// Missing tracks of the library album (`album_artist`, `album`); None when the album has no source.
/// `refresh = true` re-reads the album from the platform (`album_preview`, cached in memory for 1 h) and
/// updates the stored source; otherwise the stored track list is used.
#[command]
pub async fn get_missing_tracks(
    db: State<'_, tokio::sync::Mutex<Database>>,
    album_artist: String,
    album: String,
    refresh: Option<bool>,
) -> Result<Option<MissingTracks>, String> {
    let (sources, album_songs, library_refs, meta) = {
        let guard = db.lock().await;
        let sources = guard.list_album_sources_for_artist(&album_artist).await.map_err(db_error)?;
        if sources.is_empty() {
            return Ok(None);
        }
        let album_songs = guard
            .list_album_song_source_refs(&album_artist, &album)
            .await
            .map_err(db_error)?;
        let library_refs = guard.list_song_source_refs().await.map_err(db_error)?;
        let meta = guard.list_song_meta().await.map_err(db_error)?;
        (sources, album_songs, library_refs, meta)
    };
    let Some(row) = match_source(&sources, &album_artist, &album, &album_songs).cloned() else {
        return Ok(None);
    };
    let index = scoped_index(&LibraryIndex::from_rows(&library_refs), &meta, &album_artist, &album);

    if refresh == Some(true) {
        let platform = platform_from_str(&row.platform)
            .filter(|p| *p != Platform::Local)
            .ok_or_else(|| format!("Unknown platform \"{}\" for this album.", row.platform))?;
        // network: no lock held here
        let preview = cached_preview(platform, &row.album_url).await?;
        let fresh = refreshed_source(&row, &preview, unix_now())
            .ok_or_else(|| "The platform returned no downloadable tracks for this album.".to_string())?;
        {
            let guard = db.lock().await;
            if let Err(e) = guard.upsert_album_source(&fresh).await {
                // The answer below is still correct, only the stored copy stays old.
                eprintln!("[library_index] could not store the refreshed album source: {e}");
            }
        }
        let tracks = row_tracks_of_new(&fresh);
        return Ok(build_missing(&row, &tracks, fresh.cover_url, &index));
    }

    Ok(build_missing(&row, &row_tracks(&row), row.cover_url.clone(), &index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_index::status::platform_to_str;
    use crate::utils::make_canonical_slug;

    #[test]
    fn scoped_index_only_knows_the_names_of_its_own_album() {
        let meta: Vec<SongMeta> = vec![
            ("Artist".into(), "Other Album".into(), "Big Jacket".into(), make_canonical_slug("Artist", "Big Jacket"), None),
            ("Artist".into(), "The Album".into(), "Good Sunday".into(), make_canonical_slug("Artist", "Good Sunday"), None),
        ];
        let index = scoped_index(&LibraryIndex::default(), &meta, "artist", "the album");
        assert!(index.contains_by_meta("Artist", "Good Sunday"));
        assert!(!index.contains_by_meta("Artist", "Big Jacket"));
        assert!(!index.contains_by_fuzzy_title("Artist", "Big Jacket"));
        assert!(index.contains_by_fuzzy_title("Artist", "Good Sunday"));
    }

    #[test]
    fn scoped_index_numbered_titles_come_from_its_own_album_only() {
        let meta: Vec<SongMeta> = vec![
            ("Artist".into(), "The Album".into(), "Good Sunday".into(), make_canonical_slug("Artist", "Good Sunday"), Some(1)),
            ("Artist".into(), "Other Album".into(), "Big Jacket".into(), make_canonical_slug("Artist", "Big Jacket"), Some(2)),
            ("Artist".into(), "Other Album".into(), "Other Song".into(), make_canonical_slug("Artist", "Other Song"), Some(1)),
        ];
        let index = scoped_index(&LibraryIndex::default(), &meta, "artist", "the album");
        assert!(index.contains_numbered(Some(1), "Good Sunday (Remastered)"));
        assert!(!index.contains_numbered(Some(2), "Big Jacket"), "a song of another album does not count");
        assert!(!index.contains_numbered(Some(1), "Other Song"), "a same-numbered song of another album does not count");
    }

    #[test]
    fn scoped_index_skips_songs_without_a_track_number() {
        let meta: Vec<SongMeta> = vec![(
            "Artist".into(),
            "The Album".into(),
            "Good Sunday".into(),
            make_canonical_slug("Artist", "Good Sunday"),
            None,
        )];
        let index = scoped_index(&LibraryIndex::default(), &meta, "Artist", "The Album");
        assert!(index.numbered.is_empty());
        assert!(!index.contains_numbered(Some(1), "Good Sunday"));
        assert!(index.contains_by_meta("Artist", "Good Sunday"), "the song is still known by its name");
    }

    fn remote(platform: Platform, url: &str) -> RemoteRef {
        RemoteRef { platform, url: url.to_string(), artist: None, title: None, album: None }
    }

    fn song(source: &str, url: Option<&str>, item_id: Option<&str>) -> SongSourceRef {
        SongSourceRef {
            source: source.to_string(),
            source_url: url.map(str::to_string),
            source_item_id: item_id.map(str::to_string),
        }
    }

    fn album_song(album: &str, source: &str, url: &str) -> AlbumSongSourceRef {
        AlbumSongSourceRef {
            album: album.to_string(),
            source: source.to_string(),
            source_url: Some(url.to_string()),
            source_item_id: None,
        }
    }

    fn info(index: usize, url: &str) -> AlbumTrackInfo {
        AlbumTrackInfo {
            index,
            title: format!("t{index}"),
            artists: vec![],
            track_number: Some(index as i32 + 1),
            url: url.to_string(),
            available: true,
            note: None,
            duration_ms: Some(1000),
        }
    }

    fn row(id: i64, artist: &str, album: &str, platform: Platform, urls: &[&str]) -> AlbumSourceRow {
        let tracks: Vec<AlbumTrackInfo> = urls.iter().enumerate().map(|(i, u)| info(i, u)).collect();
        AlbumSourceRow {
            id,
            platform: platform_to_str(platform).to_string(),
            album_url: format!("https://src.test/{id}"),
            album_key: format!("src.test/{id}"),
            album_artist: artist.to_string(),
            album: album.to_string(),
            cover_url: Some("https://img/c.jpg".to_string()),
            year: None,
            track_urls: serde_json::to_string(&urls).unwrap(),
            tracks_json: Some(serde_json::to_string(&tracks).unwrap()),
            fetched_at: 1,
        }
    }

    const T1: &str = "https://a.bandcamp.com/track/1";
    const T2: &str = "https://a.bandcamp.com/track/2";
    const T3: &str = "https://a.bandcamp.com/track/3";

    // ---- track_states

    #[test]
    fn track_states_answers_every_url_as_sent() {
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        let items = vec![
            remote(Platform::Bandcamp, "https://a.bandcamp.com/track/1/?from=embed"),
            remote(Platform::Bandcamp, T2),
        ];
        let states = track_states(&items, &index, &[]);
        assert_eq!(states.len(), 2);
        assert_eq!(states["https://a.bandcamp.com/track/1/?from=embed"], DownloadState::Downloaded);
        assert_eq!(states[T2], DownloadState::None);
    }

    /// A track of "Black Sabbath" named "Iron Man" on `album`, at a url the library does not know.
    fn named(artist: &str, title: &str, album: Option<&str>) -> RemoteRef {
        RemoteRef {
            platform: Platform::Bandcamp,
            url: "https://a.bandcamp.com/track/iron".to_string(),
            artist: Some(artist.to_string()),
            title: Some(title.to_string()),
            album: album.map(str::to_string),
        }
    }

    /// The library has the studio "Iron Man" of the album "Paranoid" (its slug is in the index and in the meta).
    fn studio_iron_man() -> (LibraryIndex, Vec<SongMeta>) {
        let slug = make_canonical_slug("Black Sabbath", "Iron Man");
        let index = LibraryIndex::default().with_slugs([slug.clone()]);
        let meta = vec![("Black Sabbath".to_string(), "Paranoid".to_string(), "Iron Man".to_string(), slug, None)];
        (index, meta)
    }

    fn state_of(item: &RemoteRef, index: &LibraryIndex, meta: &[SongMeta]) -> DownloadState {
        track_states(std::slice::from_ref(item), index, meta)[&item.url]
    }

    #[test]
    fn live_track_of_another_album_is_not_downloaded() {
        let (index, meta) = studio_iron_man();
        assert_eq!(state_of(&named("Black Sabbath", "Iron Man", Some("Deep Black")), &index, &meta), DownloadState::None);
    }

    #[test]
    fn track_of_the_library_album_is_downloaded() {
        let (index, meta) = studio_iron_man();
        assert_eq!(state_of(&named("Black Sabbath", "Iron Man", Some("Paranoid")), &index, &meta), DownloadState::Downloaded);
    }

    #[test]
    fn album_name_is_compared_without_brackets_and_case() {
        let (index, meta) = studio_iron_man();
        assert_eq!(
            state_of(&named("Black Sabbath", "Iron Man", Some("Paranoid (Remastered)")), &index, &meta),
            DownloadState::Downloaded
        );
        assert_eq!(state_of(&named("Black Sabbath", "Iron Man", Some("PARANOID")), &index, &meta), DownloadState::Downloaded);
    }

    #[test]
    fn album_made_only_of_brackets_counts_as_no_album() {
        let (index, meta) = studio_iron_man();
        assert_eq!(state_of(&named("Black Sabbath", "Iron Man", Some("(Live)")), &index, &meta), DownloadState::Downloaded);
    }

    #[test]
    fn track_without_album_matches_the_library_by_name_as_before() {
        let (index, meta) = studio_iron_man();
        assert_eq!(state_of(&named("Black Sabbath", "Iron Man", None), &index, &meta), DownloadState::Downloaded);
    }

    #[test]
    fn url_match_is_downloaded_whatever_the_album() {
        let slug = make_canonical_slug("Black Sabbath", "Iron Man");
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]).with_slugs([slug.clone()]);
        let meta = vec![("Black Sabbath".to_string(), "Paranoid".to_string(), "Iron Man".to_string(), slug, None)];
        let mut item = named("Black Sabbath", "Iron Man", Some("Deep Black"));
        item.url = T1.to_string();
        assert_eq!(state_of(&item, &index, &meta), DownloadState::Downloaded);
    }

    #[test]
    fn multi_artist_track_matches_by_its_first_artist_within_the_album() {
        let (index, meta) = studio_iron_man();
        let credited = "Black Sabbath, Ozzy Osbourne";
        assert_eq!(state_of(&named(credited, "Iron Man", Some("Paranoid")), &index, &meta), DownloadState::Downloaded);
        assert_eq!(state_of(&named(credited, "Iron Man", Some("Deep Black")), &index, &meta), DownloadState::None);
    }

    #[test]
    fn track_of_the_library_album_by_another_artist_is_not_downloaded() {
        let (index, meta) = studio_iron_man();
        assert_eq!(state_of(&named("Cover Band", "Iron Man", Some("Paranoid")), &index, &meta), DownloadState::None);
    }

    #[test]
    fn track_states_of_an_empty_request_is_empty() {
        assert!(track_states(&[], &LibraryIndex::default(), &[]).is_empty());
    }

    #[test]
    fn track_states_handles_garbage_urls_and_youtube_ids() {
        let index = LibraryIndex::from_rows(&[song("youtube", None, Some("abc"))]);
        let items = vec![remote(Platform::Youtube, "https://youtu.be/abc"), remote(Platform::Youtube, "garbage")];
        let states = track_states(&items, &index, &[]);
        assert_eq!(states["https://youtu.be/abc"], DownloadState::Downloaded);
        assert_eq!(states["garbage"], DownloadState::None);
    }

    // ---- album_statuses

    fn sources_map(rows: Vec<AlbumSourceRow>) -> HashMap<String, AlbumSourceRow> {
        rows.into_iter().map(|r| (r.album_key.clone(), r)).collect()
    }

    #[test]
    fn album_statuses_none_partial_downloaded() {
        let sources = sources_map(vec![
            row(1, "A", "None", Platform::Bandcamp, &[T1, T2]),
            row(2, "A", "Partial", Platform::Bandcamp, &[T1, T2]),
            row(3, "A", "Done", Platform::Bandcamp, &[T1]),
        ]);
        let items = vec![
            remote(Platform::Bandcamp, "https://src.test/1"),
            remote(Platform::Bandcamp, "https://src.test/2"),
            remote(Platform::Bandcamp, "https://src.test/3"),
        ];
        // the library has T1 only: album 1 and 2 are partial, 3 complete
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        let status = album_statuses(&items, &sources, &index);
        assert_eq!(status["https://src.test/1"], AlbumStatus { state: DownloadState::Partial, owned: 1, total: 2 });
        assert_eq!(status["https://src.test/3"], AlbumStatus { state: DownloadState::Downloaded, owned: 1, total: 1 });

        let empty = album_statuses(&items, &sources, &LibraryIndex::default());
        assert_eq!(empty["https://src.test/2"], AlbumStatus { state: DownloadState::None, owned: 0, total: 2 });
    }

    #[test]
    fn album_statuses_unknown_album_is_none_with_zero_counts() {
        let status = album_statuses(
            &[remote(Platform::Bandcamp, "https://nope/album"), remote(Platform::Bandcamp, "")],
            &HashMap::new(),
            &LibraryIndex::default(),
        );
        assert_eq!(status["https://nope/album"], AlbumStatus { state: DownloadState::None, owned: 0, total: 0 });
        assert_eq!(status[""].state, DownloadState::None);
    }

    #[test]
    fn album_statuses_look_the_album_up_by_normalized_key() {
        let mut r = row(1, "A", "X", Platform::Bandcamp, &[T1]);
        r.album_key = "artist.bandcamp.com/album/x".to_string();
        let sources = sources_map(vec![r]);
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        let status = album_statuses(
            &[remote(Platform::Bandcamp, "https://ARTIST.bandcamp.com/album/x/?from=embed")],
            &sources,
            &index,
        );
        assert_eq!(status["https://ARTIST.bandcamp.com/album/x/?from=embed"].state, DownloadState::Downloaded);
    }

    // ---- artist_album_sources

    #[test]
    fn artist_album_sources_pairs_library_albums_with_sources() {
        let sources = vec![row(1, "Artist", "Album", Platform::Bandcamp, &[T1, T2, T3])];
        let songs = vec![album_song("Album", "bandcamp", T1), album_song("Album", "bandcamp", T3)];
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None), song("bandcamp", Some(T3), None)]);
        let out = artist_album_sources("Artist", &sources, &songs, &index);
        assert_eq!(
            out,
            vec![LibraryAlbumSource {
                album: "Album".to_string(),
                platform: Platform::Bandcamp,
                source_url: "https://src.test/1".to_string(),
                cover_url: Some("https://img/c.jpg".to_string()),
                total: 3,
                owned: 2,
                missing: 1,
            }]
        );
    }

    #[test]
    fn artist_album_sources_leave_albums_without_a_source_out() {
        let sources = vec![row(1, "Artist", "Album", Platform::Bandcamp, &[T1])];
        let songs = vec![
            album_song("Album", "bandcamp", T1),
            album_song("Local Album", "bandcamp", "https://elsewhere/track"),
            album_song("", "bandcamp", "https://elsewhere/orphan"),
        ];
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        let out = artist_album_sources("Artist", &sources, &songs, &index);
        assert_eq!(out.iter().map(|a| a.album.as_str()).collect::<Vec<_>>(), vec!["Album"]);
    }

    #[test]
    fn artist_album_sources_use_the_library_title_after_a_rename() {
        let sources = vec![row(1, "Artist", "Original", Platform::Bandcamp, &[T1, T2])];
        let songs = vec![album_song("Renamed", "bandcamp", T1)];
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        let out = artist_album_sources("Artist", &sources, &songs, &index);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].album, "Renamed");
        assert_eq!((out[0].owned, out[0].missing), (1, 1));
    }

    #[test]
    fn artist_album_sources_sort_by_title_and_merge_case_variants() {
        let sources = vec![
            row(1, "Artist", "b side", Platform::Bandcamp, &[T1]),
            row(2, "Artist", "A Side", Platform::Bandcamp, &[T2]),
        ];
        let songs = vec![album_song("B Side", "bandcamp", T1), album_song("b side", "bandcamp", T1), album_song("A Side", "bandcamp", T2)];
        let out = artist_album_sources("Artist", &sources, &songs, &LibraryIndex::default());
        assert_eq!(out.iter().map(|a| a.album.as_str()).collect::<Vec<_>>(), vec!["A Side", "B Side"]);
    }

    #[test]
    fn artist_album_sources_without_sources_or_songs_is_empty() {
        let index = LibraryIndex::default();
        assert!(artist_album_sources("A", &[], &[album_song("X", "bandcamp", T1)], &index).is_empty());
        assert!(artist_album_sources("A", &[row(1, "A", "X", Platform::Bandcamp, &[T1])], &[], &index).is_empty());
    }

    #[test]
    fn artist_album_sources_skip_sources_without_tracks() {
        let sources = vec![row(1, "A", "X", Platform::Bandcamp, &[])];
        let out = artist_album_sources("A", &sources, &[album_song("X", "bandcamp", T1)], &LibraryIndex::default());
        assert!(out.is_empty());
    }

    /// A stored source whose tracks have titles (the slug fallback needs them).
    fn titled_row(artist: &str, album: &str, tracks: &[(&str, &str)]) -> AlbumSourceRow {
        let urls: Vec<&str> = tracks.iter().map(|(_, u)| *u).collect();
        let infos: Vec<AlbumTrackInfo> = tracks
            .iter()
            .enumerate()
            .map(|(i, (title, url))| AlbumTrackInfo { title: title.to_string(), ..info(i, url) })
            .collect();
        let mut r = row(1, artist, album, Platform::Bandcamp, &urls);
        r.tracks_json = Some(serde_json::to_string(&infos).unwrap());
        r
    }

    #[test]
    fn artist_album_sources_list_an_album_whose_songs_have_no_source_refs() {
        let sources = vec![titled_row("Artist", "Album", &[("Track One", T1), ("Track Two", T2)])];
        let songs = vec![AlbumSongSourceRef {
            album: "Album".to_string(),
            source: "local".to_string(),
            source_url: None,
            source_item_id: None,
        }];
        let out = artist_album_sources("Artist", &sources, &songs, &LibraryIndex::default());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].album, "Album");
        assert_eq!(out[0].source_url, "https://src.test/1");
        assert_eq!((out[0].total, out[0].owned, out[0].missing), (2, 0, 2));
    }

    #[test]
    fn artist_album_sources_count_tracks_owned_by_slug_only() {
        let sources = vec![titled_row(
            "Artist",
            "Album",
            &[("Track One", T1), ("Track Two", T2), ("Track Three", T3)],
        )];
        let songs = vec![AlbumSongSourceRef {
            album: "Album".to_string(),
            source: "local".to_string(),
            source_url: None,
            source_item_id: None,
        }];
        let index = LibraryIndex::default().with_slugs([make_canonical_slug("Artist", "Track One")]);
        let out = artist_album_sources("Artist", &sources, &songs, &index);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].total, out[0].owned, out[0].missing), (3, 1, 2));
    }

    // ---- build_missing

    #[test]
    fn build_missing_lists_only_the_missing_tracks() {
        let r = row(1, "Artist", "Album", Platform::Bandcamp, &[T1, T2, T3]);
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T2), None)]);
        let missing = build_missing(&r, &row_tracks(&r), r.cover_url.clone(), &index).unwrap();
        assert_eq!(missing.platform, Platform::Bandcamp);
        assert_eq!(missing.source_url, "https://src.test/1");
        assert_eq!(missing.album_title, "Album");
        assert_eq!(missing.album_artist, "Artist");
        assert_eq!(missing.cover_url.as_deref(), Some("https://img/c.jpg"));
        assert_eq!(missing.tracks.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec![T1, T3]);
    }

    #[test]
    fn build_missing_is_empty_when_everything_is_owned() {
        let r = row(1, "A", "X", Platform::Bandcamp, &[T1]);
        let index = LibraryIndex::from_rows(&[song("bandcamp", Some(T1), None)]);
        assert!(build_missing(&r, &row_tracks(&r), None, &index).unwrap().tracks.is_empty());
    }

    #[test]
    fn build_missing_refuses_unknown_and_local_platforms() {
        let mut r = row(1, "A", "X", Platform::Bandcamp, &[T1]);
        r.platform = "spotify".to_string();
        assert!(build_missing(&r, &row_tracks(&r), None, &LibraryIndex::default()).is_none());
        r.platform = "local".to_string();
        assert!(build_missing(&r, &row_tracks(&r), None, &LibraryIndex::default()).is_none());
    }

    // ---- serde shapes

    #[test]
    fn library_album_source_serializes_snake_case() {
        let json = serde_json::to_value(LibraryAlbumSource {
            album: "X".to_string(),
            platform: Platform::Soundcloud,
            source_url: "https://sc".to_string(),
            cover_url: None,
            total: 3,
            owned: 1,
            missing: 2,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "album": "X", "platform": "soundcloud", "source_url": "https://sc",
                "cover_url": null, "total": 3, "owned": 1, "missing": 2
            })
        );
    }

    #[test]
    fn missing_tracks_serializes_snake_case() {
        let json = serde_json::to_value(MissingTracks {
            platform: Platform::Youtube,
            source_url: "https://yt".to_string(),
            album_title: "T".to_string(),
            album_artist: "A".to_string(),
            cover_url: Some("c".to_string()),
            tracks: vec![info(0, T1)],
        })
        .unwrap();
        assert_eq!(json["platform"], "youtube");
        assert_eq!(json["source_url"], "https://yt");
        assert_eq!(json["album_title"], "T");
        assert_eq!(json["album_artist"], "A");
        assert_eq!(json["cover_url"], "c");
        assert_eq!(json["tracks"][0]["track_number"], 1);
        assert_eq!(json["tracks"][0]["duration_ms"], 1000);
    }

    // ---- source_by_title

    #[test]
    fn source_by_title_exact_title_returns_that_row() {
        let rows = vec![
            row(1, "Aphex Twin", "Drukqs", Platform::Bandcamp, &[T1]),
            row(2, "Aphex Twin", "Syro", Platform::Bandcamp, &[T2]),
        ];
        let found = source_by_title(&rows, "Syro").expect("exact title should match");
        assert_eq!(found.id, 2);
    }

    #[test]
    fn source_by_title_ignores_case_and_punctuation() {
        let rows = vec![row(1, "Aphex Twin", "Syro", Platform::Bandcamp, &[T1])];
        assert_eq!(source_by_title(&rows, "syro").map(|r| r.id), Some(1));
        assert_eq!(source_by_title(&rows, "  SYRO! ").map(|r| r.id), Some(1));
    }

    #[test]
    fn source_by_title_ignores_bracketed_suffix_in_the_album() {
        let rows = vec![row(
            7,
            "Aphex Twin",
            "Selected Ambient Works Volume II (Expanded Edition)",
            Platform::Bandcamp,
            &[T1],
        )];
        let found = source_by_title(&rows, "Selected Ambient Works Volume II");
        assert_eq!(found.map(|r| r.id), Some(7));
    }

    #[test]
    fn source_by_title_different_title_returns_none() {
        let rows = vec![row(1, "Aphex Twin", "Drukqs", Platform::Bandcamp, &[T1])];
        assert!(source_by_title(&rows, "Syro").is_none());
    }

    #[test]
    fn source_by_title_empty_or_bracket_only_title_returns_none() {
        // Rows whose album also normalizes to "" would match without the empty-title guard.
        let rows = vec![
            row(1, "Aphex Twin", "", Platform::Bandcamp, &[T1]),
            row(2, "Aphex Twin", "(live)", Platform::Bandcamp, &[T2]),
        ];
        assert!(source_by_title(&rows, "").is_none());
        assert!(source_by_title(&rows, "(live)").is_none());
    }

    #[test]
    fn source_by_title_first_matching_row_in_slice_order_wins() {
        let rows = vec![
            row(10, "Aphex Twin", "Syro", Platform::Bandcamp, &[T1]),
            row(11, "Aphex Twin", "Syro (Deluxe)", Platform::Bandcamp, &[T2]),
            row(12, "Other Artist", "Syro", Platform::Bandcamp, &[T3]),
        ];
        assert_eq!(source_by_title(&rows, "Syro").map(|r| r.id), Some(10));
    }
}
