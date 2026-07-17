use crate::lyrics::LyricsSource;
use crate::types::{Album, ArtistSummary, Song};
use sqlx::sqlite::SqlitePool;

pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &str) -> Self {
        let pool = SqlitePool::connect(&format!("sqlite:{db_path}"))
            .await
            .expect("Failed to connect to database");
        Self { pool }
    }

    pub async fn get_all_songs(&self) -> Result<Vec<Song>, sqlx::Error> {
        sqlx::query_as::<_, Song>(
            "SELECT *
             FROM songs
             ORDER BY album_artist ASC, album ASC, track_number ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_artists_summary(&self) -> Result<Vec<ArtistSummary>, sqlx::Error> {
        // MAX(cover_url) just to grab one valid album cover
        let artists = sqlx::query_as::<_, ArtistSummary>(
            "SELECT
                album_artist AS name,
                COUNT(DISTINCT album) AS album_count,
                MAX(cover_url) AS cover_url
             FROM songs
             GROUP BY album_artist
             ORDER BY album_artist ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(artists)
    }

    pub async fn get_artist_albums(&self, artist_name: &str) -> Result<Vec<Album>, sqlx::Error> {
        let songs = sqlx::query_as::<_, Song>(
            "SELECT * FROM songs WHERE album_artist = ? ORDER BY album ASC, track_number ASC",
        )
        .bind(artist_name)
        .fetch_all(&self.pool)
        .await?;

        let mut album_map: std::collections::BTreeMap<String, Album> =
            std::collections::BTreeMap::new();

        for song in songs {
            let entry = album_map.entry(song.album.clone()).or_insert(Album {
                title: song.album.clone(),
                album_artist: song.album_artist.clone(),
                cover_url: song.cover_url.clone(),
                songs: Vec::new(),
            });
            entry.songs.push(song);
        }

        let albums = album_map.into_values().collect();

        Ok(albums)
    }

    pub async fn update_external_cover(&self, id: i64, url: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE songs SET external_cover_url = ? WHERE id = ?")
            .bind(url)
            .bind(id)
            .execute(&self.pool)
            .await?;

        // println!("SAVED COVER URL: {url:?}");

        Ok(())
    }

    pub async fn update_song_lyrics(
        &self,
        id: i64,
        lyrics_source: LyricsSource,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE songs SET lyrics = ?, lyrics_source = ? WHERE id = ?")
            .bind(lyrics_source.lyrics)
            .bind(lyrics_source.source)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn insert_song(&self, song: Song) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO songs (
                path, title, artists, album_artist, album, track_number,
                duration_ms, cover_url, external_cover_url, genre, release_year,
                lyrics, lyrics_source, source, source_url, source_item_id,
                canonical_track_slug, canonical_album_slug, quality_tier
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title",
        )
        .bind(&song.path)
        .bind(&song.title)
        .bind(sqlx::types::Json(&song.artists)) // Binds the Vec<String> as JSON String
        .bind(&song.album_artist)
        .bind(&song.album)
        .bind(song.track_number)
        .bind(song.duration_ms as i64)
        .bind(&song.cover_url)
        .bind(&song.external_cover_url)
        .bind(&song.genre)
        .bind(song.release_year)
        .bind(&song.lyrics)
        .bind(&song.lyrics_source)
        .bind(song.source)
        .bind(&song.source_url)
        .bind(&song.source_item_id)
        .bind(&song.canonical_track_slug)
        .bind(&song.canonical_album_slug)
        .bind(song.quality_tier)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_songs(&self, songs: Vec<Song>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for song in songs {
            sqlx::query(
                "INSERT INTO songs (
                    path, title, artists, album_artist, album, track_number,
                    duration_ms, cover_url, external_cover_url, genre, release_year,
                    lyrics, lyrics_source, source, source_url, source_item_id,
                    canonical_track_slug, canonical_album_slug, quality_tier
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(path) DO UPDATE SET
                    title = excluded.title,
                    artists = excluded.artists,
                    album_artist = excluded.album_artist,
                    album = excluded.album,
                    track_number = excluded.track_number,
                    duration_ms = excluded.duration_ms,
                    cover_url = excluded.cover_url,
                    genre = excluded.genre,
                    release_year = excluded.release_year,
                    source = excluded.source,
                    source_url = excluded.source_url,
                    quality_tier = excluded.quality_tier
                ",
            )
            .bind(&song.path)
            .bind(&song.title)
            .bind(sqlx::types::Json(&song.artists))
            .bind(&song.album_artist)
            .bind(&song.album)
            .bind(song.track_number)
            .bind(song.duration_ms as i64)
            .bind(&song.cover_url)
            .bind(&song.external_cover_url)
            .bind(&song.genre)
            .bind(song.release_year)
            .bind(&song.lyrics)
            .bind(&song.lyrics_source)
            .bind(song.source)
            .bind(&song.source_url)
            .bind(&song.source_item_id)
            .bind(&song.canonical_track_slug)
            .bind(&song.canonical_album_slug)
            .bind(song.quality_tier)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_song_by_id(&self, id: i64) -> Result<Option<Song>, sqlx::Error> {
        let song = sqlx::query_as::<_, Song>(
            "SELECT
                id, path, title, artists, album_artist, album, track_number,
                duration_ms, cover_url, external_cover_url, genre, release_year,
                lyrics, lyrics_source, source, source_url, source_item_id,
                canonical_track_slug, canonical_album_slug, quality_tier
             FROM songs
             WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(song)
    }

    pub async fn delete_songs(&self, ids: Vec<i64>) -> Result<(), sqlx::Error> {
        if ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for id in ids {
            sqlx::query("DELETE FROM songs WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}
