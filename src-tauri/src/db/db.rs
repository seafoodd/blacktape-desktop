use crate::types::Song;
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
                "INSERT INTO songs (path, title, artist, album, track_number, duration_ms, cover_url)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET title = excluded.title"
            )
                .bind(&song.path)
                .bind(&song.title)
                .bind(&song.artist)
                .bind(&song.album)
                .bind(song.track_number)
                .bind(song.duration_ms as i64)
                .bind(&song.cover_url)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        Ok(())
    }
}