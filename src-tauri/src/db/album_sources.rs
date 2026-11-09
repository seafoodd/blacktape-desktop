//! `album_sources` table access (library-sync D5). Owner: part A. See PLAN.md (v3).
//! The schema is `schema::ALBUM_SOURCES_SCHEMA_SQL`; the url key column is `normalize_url_key(album_url)`.

use super::db::Database;

/// A stored album source row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AlbumSourceRow {
    pub id: i64,
    /// "youtube" | "bandcamp" | "soundcloud"
    pub platform: String,
    pub album_url: String,
    pub album_key: String,
    pub album_artist: String,
    pub album: String,
    pub cover_url: Option<String>,
    pub year: Option<i32>,
    /// JSON array of all downloadable track page urls, in album order.
    pub track_urls: String,
    /// JSON array of `AlbumTrackInfo` (nullable for rows written without details).
    pub tracks_json: Option<String>,
    /// Unix seconds of the last refresh.
    pub fetched_at: i64,
}

/// What the download pipeline / a refresh writes (the key is computed by the writer).
#[derive(Debug, Clone, PartialEq)]
pub struct NewAlbumSource {
    pub platform: String,
    pub album_url: String,
    pub album_key: String,
    pub album_artist: String,
    pub album: String,
    pub cover_url: Option<String>,
    pub year: Option<i32>,
    pub track_urls: Vec<String>,
    pub tracks_json: Option<String>,
    pub fetched_at: i64,
}

/// Name data of one library song (see `Database::list_song_meta`).
pub type SongMeta = (String, String, String, String);

/// The three columns of a library song that identify it on a platform.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct SongSourceRef {
    pub source: String,
    pub source_url: Option<String>,
    pub source_item_id: Option<String>,
}

/// `SongSourceRef` plus the library album the song belongs to (`''` when it has none).
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct AlbumSongSourceRef {
    pub album: String,
    pub source: String,
    pub source_url: Option<String>,
    pub source_item_id: Option<String>,
}

/// Songs that carry a remote reference (shared by the three `songs` queries). A macro so that `concat!` can
/// build static sql strings (sqlx only accepts `&'static str`).
macro_rules! has_source_ref {
    () => {
        "((source_url IS NOT NULL AND source_url <> '') OR (source_item_id IS NOT NULL AND source_item_id <> ''))"
    };
}

impl Database {
    /// Inserts or replaces (by `album_key`) the source of an album.
    pub async fn upsert_album_source(&self, src: &NewAlbumSource) -> Result<(), sqlx::Error> {
        let track_urls =
            serde_json::to_string(&src.track_urls).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        sqlx::query(
            "INSERT INTO album_sources
                (platform, album_url, album_key, album_artist, album, cover_url, year, track_urls, tracks_json, fetched_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(album_key) DO UPDATE SET
                platform = excluded.platform,
                album_url = excluded.album_url,
                album_artist = excluded.album_artist,
                album = excluded.album,
                cover_url = excluded.cover_url,
                year = excluded.year,
                track_urls = excluded.track_urls,
                tracks_json = excluded.tracks_json,
                fetched_at = excluded.fetched_at",
        )
        .bind(&src.platform)
        .bind(&src.album_url)
        .bind(&src.album_key)
        .bind(&src.album_artist)
        .bind(&src.album)
        .bind(&src.cover_url)
        .bind(src.year)
        .bind(track_urls)
        .bind(&src.tracks_json)
        .bind(src.fetched_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_album_source_by_key(
        &self,
        album_key: &str,
    ) -> Result<Option<AlbumSourceRow>, sqlx::Error> {
        sqlx::query_as::<_, AlbumSourceRow>("SELECT * FROM album_sources WHERE album_key = ?")
            .bind(album_key)
            .fetch_optional(&self.pool)
            .await
    }

    /// Sources whose `album_artist` equals `album_artist` (case-insensitive).
    pub async fn list_album_sources_for_artist(
        &self,
        album_artist: &str,
    ) -> Result<Vec<AlbumSourceRow>, sqlx::Error> {
        sqlx::query_as::<_, AlbumSourceRow>(
            "SELECT * FROM album_sources WHERE album_artist = ? COLLATE NOCASE ORDER BY id",
        )
        .bind(album_artist.trim())
        .fetch_all(&self.pool)
        .await
    }

    /// `canonical_track_slug` of every library song (the fallback for songs without a source url).
    pub async fn list_track_slugs(&self) -> Result<Vec<String>, sqlx::Error> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT canonical_track_slug FROM songs WHERE canonical_track_slug != ''")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// (album artist, album, title, canonical slug) of every library song: what a library album owns by name.
    pub async fn list_song_meta(&self) -> Result<Vec<SongMeta>, sqlx::Error> {
        sqlx::query_as("SELECT album_artist, album, title, canonical_track_slug FROM songs")
            .fetch_all(&self.pool)
            .await
    }

    /// (album artist, title) of every library song, for the fuzzy title match.
    pub async fn list_song_artist_titles(&self) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as("SELECT album_artist, title FROM songs").fetch_all(&self.pool).await
    }

    /// Removes the stored sources of a library album (an album is linked to one platform album at a time).
    pub async fn delete_album_sources_for(&self, album_artist: &str, album: &str) -> Result<u64, sqlx::Error> {
        let done = sqlx::query(
            "DELETE FROM album_sources WHERE album_artist = ? COLLATE NOCASE AND album = ? COLLATE NOCASE",
        )
        .bind(album_artist.trim())
        .bind(album.trim())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// Source refs of ALL library songs that have a source_url or a source_item_id (the library index).
    pub async fn list_song_source_refs(&self) -> Result<Vec<SongSourceRef>, sqlx::Error> {
        sqlx::query_as::<_, SongSourceRef>(concat!(
            "SELECT source, source_url, source_item_id FROM songs WHERE ",
            has_source_ref!()
        ))
        .fetch_all(&self.pool)
        .await
    }

    /// Source refs of the songs of one library album (`album_artist`, `album`, both case-insensitive); songs
    /// without any remote reference are left out.
    pub async fn list_album_song_source_refs(
        &self,
        album_artist: &str,
        album: &str,
    ) -> Result<Vec<SongSourceRef>, sqlx::Error> {
        sqlx::query_as::<_, SongSourceRef>(concat!(
            "SELECT source, source_url, source_item_id FROM songs
             WHERE album_artist = ? COLLATE NOCASE AND album = ? COLLATE NOCASE AND ",
            has_source_ref!()
        ))
        .bind(album_artist.trim())
        .bind(album.trim())
        .fetch_all(&self.pool)
        .await
    }

    /// Source refs (with their library album) of every song of one artist, also songs without a remote reference
    /// (albums added from local files are listed so they can be linked to a platform album).
    pub async fn list_artist_song_source_refs(
        &self,
        album_artist: &str,
    ) -> Result<Vec<AlbumSongSourceRef>, sqlx::Error> {
        sqlx::query_as::<_, AlbumSongSourceRef>(concat!(
            "SELECT COALESCE(album, '') AS album, source, source_url, source_item_id FROM songs
             WHERE album_artist = ? COLLATE NOCASE"
        ))
        .bind(album_artist.trim())
        .fetch_all(&self.pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::db::tests::setup_test_db;

    async fn db() -> Database {
        let db = setup_test_db().await;
        db.ensure_album_sources_schema().await.unwrap();
        db
    }

    fn source(key: &str, artist: &str, album: &str) -> NewAlbumSource {
        NewAlbumSource {
            platform: "bandcamp".to_string(),
            album_url: format!("https://{key}"),
            album_key: key.to_string(),
            album_artist: artist.to_string(),
            album: album.to_string(),
            cover_url: Some("https://img/c.jpg".to_string()),
            year: Some(2020),
            track_urls: vec!["https://a/1".to_string(), "https://a/2".to_string()],
            tracks_json: Some("[]".to_string()),
            fetched_at: 100,
        }
    }

    async fn add_song(db: &Database, path: &str, artist: &str, album: Option<&str>, source: &str, url: Option<&str>, item_id: Option<&str>) {
        sqlx::query(
            "INSERT INTO songs (path, title, album_artist, album, duration_ms, source, source_url, source_item_id)
             VALUES (?, 't', ?, ?, 1000, ?, ?, ?)",
        )
        .bind(path)
        .bind(artist)
        .bind(album)
        .bind(source)
        .bind(url)
        .bind(item_id)
        .execute(&db.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn upsert_then_get_by_key_round_trips() {
        let db = db().await;
        db.upsert_album_source(&source("a.bandcamp.com/album/x", "Artist", "X")).await.unwrap();

        let row = db.get_album_source_by_key("a.bandcamp.com/album/x").await.unwrap().expect("row");
        assert_eq!(row.platform, "bandcamp");
        assert_eq!(row.album_url, "https://a.bandcamp.com/album/x");
        assert_eq!(row.album_artist, "Artist");
        assert_eq!(row.album, "X");
        assert_eq!(row.cover_url.as_deref(), Some("https://img/c.jpg"));
        assert_eq!(row.year, Some(2020));
        assert_eq!(row.track_urls, r#"["https://a/1","https://a/2"]"#);
        assert_eq!(row.tracks_json.as_deref(), Some("[]"));
        assert_eq!(row.fetched_at, 100);
    }

    #[tokio::test]
    async fn get_by_key_is_none_when_missing() {
        let db = db().await;
        assert!(db.get_album_source_by_key("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn upsert_replaces_all_columns_of_the_same_key() {
        let db = db().await;
        db.upsert_album_source(&source("k", "Artist", "X")).await.unwrap();
        let mut newer = source("k", "Other Artist", "X (Deluxe)");
        newer.platform = "soundcloud".to_string();
        newer.album_url = "https://sc/other".to_string();
        newer.cover_url = None;
        newer.year = None;
        newer.track_urls = vec!["https://a/9".to_string()];
        newer.tracks_json = None;
        newer.fetched_at = 999;
        db.upsert_album_source(&newer).await.unwrap();

        let rows = db.list_album_sources_for_artist("Other Artist").await.unwrap();
        assert_eq!(rows.len(), 1, "same key must not create a second row");
        let row = &rows[0];
        assert_eq!(row.platform, "soundcloud");
        assert_eq!(row.album_url, "https://sc/other");
        assert_eq!(row.album, "X (Deluxe)");
        assert_eq!(row.cover_url, None);
        assert_eq!(row.year, None);
        assert_eq!(row.track_urls, r#"["https://a/9"]"#);
        assert_eq!(row.tracks_json, None);
        assert_eq!(row.fetched_at, 999);
        assert!(db.list_album_sources_for_artist("Artist").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upsert_keeps_the_row_id() {
        let db = db().await;
        db.upsert_album_source(&source("k", "A", "X")).await.unwrap();
        let first = db.get_album_source_by_key("k").await.unwrap().unwrap().id;
        db.upsert_album_source(&source("k", "A", "X")).await.unwrap();
        assert_eq!(db.get_album_source_by_key("k").await.unwrap().unwrap().id, first);
    }

    #[tokio::test]
    async fn list_for_artist_is_case_insensitive_and_exact() {
        let db = db().await;
        db.upsert_album_source(&source("k1", "The Artist", "One")).await.unwrap();
        db.upsert_album_source(&source("k2", "the artist", "Two")).await.unwrap();
        db.upsert_album_source(&source("k3", "The Artist Band", "Three")).await.unwrap();

        let rows = db.list_album_sources_for_artist("THE ARTIST").await.unwrap();
        let albums: Vec<&str> = rows.iter().map(|r| r.album.as_str()).collect();
        assert_eq!(albums, vec!["One", "Two"]);
        assert!(db.list_album_sources_for_artist("nobody").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn song_source_refs_only_contain_songs_with_a_reference() {
        let db = db().await;
        add_song(&db, "p1", "A", Some("X"), "bandcamp", Some("https://a/1"), None).await;
        add_song(&db, "p2", "A", Some("X"), "youtube", None, Some("vid")).await;
        add_song(&db, "p3", "A", Some("X"), "local", None, None).await;
        add_song(&db, "p4", "A", Some("X"), "local", Some(""), Some("")).await;

        let mut refs = db.list_song_source_refs().await.unwrap();
        refs.sort_by(|a, b| a.source.cmp(&b.source));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].source, "bandcamp");
        assert_eq!(refs[0].source_url.as_deref(), Some("https://a/1"));
        assert_eq!(refs[1].source, "youtube");
        assert_eq!(refs[1].source_item_id.as_deref(), Some("vid"));
    }

    #[tokio::test]
    async fn song_source_refs_of_an_empty_library_are_empty() {
        let db = db().await;
        assert!(db.list_song_source_refs().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn album_song_refs_filter_by_artist_and_album_case_insensitively() {
        let db = db().await;
        add_song(&db, "p1", "Artist", Some("Album"), "bandcamp", Some("https://a/1"), None).await;
        add_song(&db, "p2", "Artist", Some("Other"), "bandcamp", Some("https://a/2"), None).await;
        add_song(&db, "p3", "Someone", Some("Album"), "bandcamp", Some("https://a/3"), None).await;
        add_song(&db, "p4", "Artist", Some("Album"), "local", None, None).await;

        let refs = db.list_album_song_source_refs("ARTIST", "album").await.unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_url.as_deref(), Some("https://a/1"));
    }

    #[tokio::test]
    async fn artist_song_refs_carry_the_album_and_use_empty_for_null() {
        let db = db().await;
        add_song(&db, "p1", "Artist", Some("Album"), "bandcamp", Some("https://a/1"), None).await;
        add_song(&db, "p2", "Artist", None, "bandcamp", Some("https://a/2"), None).await;
        add_song(&db, "p3", "Other", Some("Album"), "bandcamp", Some("https://a/3"), None).await;

        let mut refs = db.list_artist_song_source_refs("artist").await.unwrap();
        refs.sort_by(|a, b| a.album.cmp(&b.album));
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].album, "");
        assert_eq!(refs[1].album, "Album");
    }

    #[tokio::test]
    async fn artist_song_refs_include_songs_without_a_remote_reference() {
        let db = db().await;
        add_song(&db, "p1", "Artist", Some("Local Album"), "local", None, None).await;
        add_song(&db, "p2", "Artist", Some("Local Album"), "bandcamp", Some("https://a/1"), None).await;
        add_song(&db, "p3", "Other", Some("Local Album"), "local", None, None).await;

        let refs = db.list_artist_song_source_refs("ARTIST").await.unwrap();
        assert_eq!(refs.len(), 2, "the other artist's song is left out");
        let local = refs.iter().find(|r| r.source == "local").expect("local song is listed");
        assert_eq!(local.album, "Local Album");
        assert_eq!(local.source_url, None);
        assert_eq!(local.source_item_id, None);
    }

    #[tokio::test]
    async fn ensure_schema_is_idempotent() {
        let db = db().await;
        db.upsert_album_source(&source("k", "A", "X")).await.unwrap();
        db.ensure_album_sources_schema().await.unwrap();
        assert!(db.get_album_source_by_key("k").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn delete_album_sources_removes_only_that_album_case_insensitively() {
        let db = db().await;
        db.upsert_album_source(&source("k1", "Artist", "Album")).await.unwrap();
        db.upsert_album_source(&source("k2", "artist", "ALBUM")).await.unwrap();
        db.upsert_album_source(&source("k3", "Artist", "Other")).await.unwrap();
        db.upsert_album_source(&source("k4", "Other Artist", "Album")).await.unwrap();

        let deleted = db.delete_album_sources_for("  ARTIST ", " album ").await.unwrap();
        assert_eq!(deleted, 2);
        assert!(db.get_album_source_by_key("k1").await.unwrap().is_none());
        assert!(db.get_album_source_by_key("k2").await.unwrap().is_none());
        assert!(db.get_album_source_by_key("k3").await.unwrap().is_some());
        assert!(db.get_album_source_by_key("k4").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn delete_album_sources_returns_zero_when_nothing_matches() {
        let db = db().await;
        db.upsert_album_source(&source("k1", "Artist", "Album")).await.unwrap();
        assert_eq!(db.delete_album_sources_for("Artist", "Missing").await.unwrap(), 0);
        assert_eq!(db.delete_album_sources_for("Nobody", "Album").await.unwrap(), 0);
        assert!(db.get_album_source_by_key("k1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn list_song_artist_titles_returns_album_artist_and_title_pairs() {
        let db = db().await;
        assert!(db.list_song_artist_titles().await.unwrap().is_empty());

        for (path, title, artist) in [("p1", "One", "Artist"), ("p2", "Two", "Other Artist")] {
            sqlx::query(
                "INSERT INTO songs (path, title, album_artist, album, duration_ms, source)
                 VALUES (?, ?, ?, 'Album', 1000, 'local')",
            )
            .bind(path)
            .bind(title)
            .bind(artist)
            .execute(&db.pool)
            .await
            .unwrap();
        }

        let mut pairs = db.list_song_artist_titles().await.unwrap();
        pairs.sort();
        assert_eq!(
            pairs,
            vec![
                ("Artist".to_string(), "One".to_string()),
                ("Other Artist".to_string(), "Two".to_string()),
            ]
        );
    }
}
