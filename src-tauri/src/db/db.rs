use crate::types::{ArtistSummary, Song};
use sqlx::sqlite::SqlitePool;

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(db_path: &str) -> Self {
        let pool = SqlitePool::connect(&format!("sqlite:{}", db_path))
            .await
            .expect("Failed to connect to database");
        Self { pool }
    }

    pub async fn get_all_songs(&self) -> Result<Vec<Song>, sqlx::Error> {
        sqlx::query_as::<_, Song>(
            "SELECT
                id, path, title, artist, album, track_number,
                duration_ms, cover_url, genre, release_year
             FROM songs
             ORDER BY artist ASC, album ASC, track_number ASC"
        )
            .fetch_all(&self.pool)
            .await
    }

    pub async fn get_artists_summary(&self) -> Result<Vec<ArtistSummary>, sqlx::Error> {
        // Query to group by artist name
        // We use MAX(cover_url) just to grab one valid image from their catalog
        let artists = sqlx::query_as::<_, ArtistSummary>(
            "SELECT
                artist AS name,
                COUNT(DISTINCT album) AS album_count,
                MAX(cover_url) AS cover_url
             FROM songs
             GROUP BY artist
             ORDER BY artist ASC"
        )
            .fetch_all(&self.pool)
            .await?;

        Ok(artists)
    }

    pub async fn insert_song(&self, song: Song) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO songs (path, title, artist, album, track_number, duration_ms, cover_url)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title"
        )
            .bind(song.path)
            .bind(song.title)
            .bind(song.artist)
            .bind(song.album)
            .bind(song.track_number)
            .bind(song.duration_ms as i64)
            .bind(song.cover_url)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn insert_songs(&self, songs: Vec<Song>) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for song in songs {
            sqlx::query(
                "INSERT INTO songs (path, title, artist, album, track_number, duration_ms, cover_url, genre, release_year)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                album = excluded.album,
                track_number = excluded.track_number,
                duration_ms = excluded.duration_ms,
                cover_url = excluded.cover_url,
                genre = excluded.genre,
                release_year = excluded.release_year"
            )
                .bind(&song.path)
                .bind(&song.title)
                .bind(&song.artist)
                .bind(&song.album)
                .bind(song.track_number)
                .bind(song.duration_ms as i64)
                .bind(&song.cover_url)
                .bind(&song.genre)
                .bind(&song.release_year)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        Ok(())
    }

    pub async fn get_song_by_id(&self, id: i64) -> Result<Option<Song>, sqlx::Error> {
        let song = sqlx::query_as::<_, Song>(
            "SELECT id, path, title, artist, album, track_number, duration_ms, cover_url, genre, release_year
             FROM songs
             WHERE id = ?"
        )
            .bind(id)
            .fetch_optional(&self.pool) // returns None if no song is found
            .await?;

        Ok(song)
    }
}