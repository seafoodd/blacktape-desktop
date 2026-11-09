//! Tauri commands of the library index (registered in lib.rs; names and shapes are FINAL). Owner: part A.
//! Never hold the database mutex across a network call (`get_missing_tracks` with refresh): every command reads
//! its rows under the lock, drops the guard and computes on plain values (the pure helpers below and in
//! `sources.rs` are what the tests cover).

use crate::db::album_sources::{AlbumSongSourceRef, AlbumSourceRow, SongSourceRef};
use crate::db::db::Database;
use crate::download::album_preview::{album_preview, AlbumPreview, AlbumTrackInfo};
use crate::library_index::normalize::normalize_url_key;
use crate::library_index::sources::{
    compute_missing, count_owned, match_source, refreshed_source, row_tracks, row_tracks_of_new, TtlCache,
};
use crate::library_index::status::{
    album_state, platform_from_str, AlbumStatus, DownloadState, LibraryIndex, RemoteRef,
};
use crate::types::Platform;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
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

/// Library state of every requested track url (`downloaded` or `none`); keyed by the url exactly as sent. Pure.
pub fn track_states(items: &[RemoteRef], index: &LibraryIndex) -> HashMap<String, DownloadState> {
    items
        .iter()
        .map(|item| {
            let state = if index.contains_track(item.platform, &item.url) {
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
    let (owned, total) = count_owned(platform, &row_tracks(row), index);
    AlbumStatus { state: album_state(owned, total), owned, total }
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
    let refs = {
        let guard = db.lock().await;
        guard.list_song_source_refs().await.map_err(db_error)?
    };
    Ok(track_states(&items, &LibraryIndex::from_rows(&refs)))
}

/// Library state of remote ALBUM urls, from `album_sources` (albums the app never downloaded are `none`).
#[command]
pub async fn get_album_status(
    db: State<'_, tokio::sync::Mutex<Database>>,
    items: Vec<RemoteRef>,
) -> Result<HashMap<String, AlbumStatus>, String> {
    let mut keys: Vec<String> = items.iter().filter_map(|i| normalize_url_key(&i.url)).collect();
    keys.sort();
    keys.dedup();

    let (sources, refs) = {
        let guard = db.lock().await;
        let mut sources = HashMap::new();
        for key in keys {
            if let Some(row) = guard.get_album_source_by_key(&key).await.map_err(db_error)? {
                sources.insert(key, row);
            }
        }
        // Without any known album there is nothing to compare, skip the library scan.
        let refs = if sources.is_empty() {
            Vec::new()
        } else {
            guard.list_song_source_refs().await.map_err(db_error)?
        };
        (sources, refs)
    };
    Ok(album_statuses(&items, &sources, &LibraryIndex::from_rows(&refs)))
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
            let (owned, total) = count_owned(platform, &row_tracks(row), index);
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
    let (sources, songs, refs) = {
        let guard = db.lock().await;
        let sources = guard.list_album_sources_for_artist(&album_artist).await.map_err(db_error)?;
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let songs = guard.list_artist_song_source_refs(&album_artist).await.map_err(db_error)?;
        let refs = guard.list_song_source_refs().await.map_err(db_error)?;
        (sources, songs, refs)
    };
    Ok(artist_album_sources(&album_artist, &sources, &songs, &LibraryIndex::from_rows(&refs)))
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
        tracks: compute_missing(platform, tracks, index),
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
    let (sources, album_songs, library_refs) = {
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
        (sources, album_songs, library_refs)
    };
    let Some(row) = match_source(&sources, &album_artist, &album, &album_songs).cloned() else {
        return Ok(None);
    };
    let index = LibraryIndex::from_rows(&library_refs);

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

    fn remote(platform: Platform, url: &str) -> RemoteRef {
        RemoteRef { platform, url: url.to_string() }
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
        let states = track_states(&items, &index);
        assert_eq!(states.len(), 2);
        assert_eq!(states["https://a.bandcamp.com/track/1/?from=embed"], DownloadState::Downloaded);
        assert_eq!(states[T2], DownloadState::None);
    }

    #[test]
    fn track_states_of_an_empty_request_is_empty() {
        assert!(track_states(&[], &LibraryIndex::default()).is_empty());
    }

    #[test]
    fn track_states_handles_garbage_urls_and_youtube_ids() {
        let index = LibraryIndex::from_rows(&[song("youtube", None, Some("abc"))]);
        let items = vec![remote(Platform::Youtube, "https://youtu.be/abc"), remote(Platform::Youtube, "garbage")];
        let states = track_states(&items, &index);
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
}
