use sqlx::sqlite::SqlitePool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The app database file name.
pub const DB_NAME: &str = "blacktape.db";
pub const DB_URL: &str = "sqlite:blacktape.db";

pub fn db_file_path(app_config_dir: &Path) -> PathBuf {
    app_config_dir.join(DB_NAME)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub description: &'static str,
    pub sql: &'static str,
}

/// Runs pending database migrations against the given SQLite pool.
///
/// Uses the exact same `_sqlx_migrations` table schema as `sqlx` and `tauri-plugin-sql`,
/// ensuring 100% backward and forward compatibility with existing user databases.
pub async fn run_migrations(pool: &SqlitePool, migrations: &[Migration]) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN NOT NULL,
            checksum BLOB NOT NULL DEFAULT x'',
            execution_time BIGINT NOT NULL DEFAULT 0
        );"
    )
    .execute(pool)
    .await?;

    let applied: HashSet<i64> = sqlx::query_scalar(
        "SELECT version FROM _sqlx_migrations WHERE success = 1"
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    for m in migrations {
        if !applied.contains(&m.version) {
            let start = std::time::Instant::now();
            let mut tx = pool.begin().await?;

            sqlx::raw_sql(m.sql).execute(&mut *tx).await?;

            let elapsed_nanos = start.elapsed().as_nanos() as i64;
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
                 VALUES (?, ?, 1, x'', ?)"
            )
            .bind(m.version)
            .bind(m.description)
            .bind(elapsed_nanos)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            tracing::info!("Applied database migration {}: {}", m.version, m.description);
        }
    }

    Ok(())
}

/// Trash tables (D10). Idempotent, so it runs both as migration 6 and from
/// `Database::ensure_trash_schema()` (the app DB is also opened by sqlx directly).
pub const TRASH_SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS trash_items (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        song_json TEXT NOT NULL,
        original_path TEXT NOT NULL,
        trash_path TEXT NOT NULL,
        size_bytes INTEGER NOT NULL,
        trashed_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS app_settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
";

/// Album sources (library-sync D5): one row per remote album the app downloaded, with the WHOLE track list of
/// that album. Idempotent, so it runs both as migration 7 and from `Database::ensure_album_sources_schema()`.
/// `album_key` is `library_index::normalize::normalize_url_key(album_url)`; `track_urls` is a JSON array of
/// all downloadable track page urls in album order; `tracks_json` is a JSON array of `AlbumTrackInfo`
/// (title, artists, number, url, duration) so the "missing songs" list works offline.
pub const ALBUM_SOURCES_SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS album_sources (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        platform TEXT NOT NULL,
        album_url TEXT NOT NULL,
        album_key TEXT NOT NULL UNIQUE,
        album_artist TEXT NOT NULL,
        album TEXT NOT NULL,
        cover_url TEXT,
        year INTEGER,
        track_urls TEXT NOT NULL DEFAULT '[]',
        tracks_json TEXT,
        fetched_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_album_sources_artist ON album_sources(album_artist COLLATE NOCASE);
";

pub const FAVORITES_SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS favorite_songs (
        song_id INTEGER PRIMARY KEY,
        date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (song_id) REFERENCES songs(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_favorite_songs_date ON favorite_songs(date_added DESC);
";

pub fn get_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "create core songs table",
            sql: "CREATE TABLE IF NOT EXISTS songs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                artist TEXT NOT NULL DEFAULT 'Unknown Artist',
                album TEXT,
                track_number INTEGER,
                duration_ms INTEGER NOT NULL,
                cover_url TEXT,
                external_cover_url TEXT,
                source_url TEXT,
                genre TEXT,
                release_year INTEGER,
                date_added DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
        },
        Migration {
            version: 2,
            description: "add lyrics field",
            sql: "ALTER TABLE songs ADD COLUMN lyrics TEXT;",
        },
        Migration {
            version: 3,
            description: "add lyrics source field",
            sql: "ALTER TABLE songs ADD COLUMN lyrics_source TEXT;",
        },
        Migration {
            version: 4,
            description: "sync schema with new song fields",
            sql: "
                ALTER TABLE songs RENAME COLUMN artist TO album_artist;
                ALTER TABLE songs ADD COLUMN artists TEXT NOT NULL DEFAULT '[]';
                ALTER TABLE songs ADD COLUMN source TEXT NOT NULL DEFAULT 'local';
                ALTER TABLE songs ADD COLUMN source_item_id TEXT;
                ALTER TABLE songs ADD COLUMN canonical_track_slug TEXT NOT NULL DEFAULT '';
                ALTER TABLE songs ADD COLUMN canonical_album_slug TEXT NOT NULL DEFAULT '';
                ALTER TABLE songs ADD COLUMN quality_tier TEXT NOT NULL DEFAULT 'standard';
            ",
        },
        Migration {
            version: 5,
            description: "create playlists and playlist songs tables",
            sql: "
                CREATE TABLE IF NOT EXISTS playlists (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    description TEXT,
                    cover_url TEXT,
                    date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
                    date_modified DATETIME DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS playlist_songs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    playlist_id INTEGER NOT NULL,
                    song_id INTEGER NOT NULL,
                    position INTEGER NOT NULL CHECK (position > 0),
                    date_added DATETIME DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY (playlist_id) REFERENCES playlists(id) ON DELETE CASCADE,
                    FOREIGN KEY (song_id) REFERENCES songs(id) ON DELETE CASCADE,
                    UNIQUE(playlist_id, position)
                );

                CREATE INDEX IF NOT EXISTS idx_playlist_songs_position ON playlist_songs(playlist_id, position);
            ",
        },
        Migration {
            version: 6,
            description: "create trash items and app settings tables",
            sql: TRASH_SCHEMA_SQL,
        },
        Migration {
            version: 7,
            description: "create album sources table",
            sql: ALBUM_SOURCES_SCHEMA_SQL,
        },
        Migration {
            version: 8,
            description: "create favorite songs table",
            sql: FAVORITES_SCHEMA_SQL,
        },
        Migration {
            version: 9,
            description: "add disc_number to songs",
            sql: "ALTER TABLE songs ADD COLUMN disc_number INTEGER;",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_file_sits_in_the_config_folder() {
        let dir = Path::new("config-dir");
        assert_eq!(db_file_path(dir), dir.join("blacktape.db"));
    }

    #[tokio::test]
    async fn run_migrations_fresh_creates_all_tables_and_records_migrations() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let migrations = get_migrations();

        run_migrations(&pool, &migrations).await.unwrap();

        // Verify all 9 migrations are recorded in _sqlx_migrations
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 9);

        // Verify key tables exist
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(tables.contains(&"songs".to_string()));
        assert!(tables.contains(&"playlists".to_string()));
        assert!(tables.contains(&"playlist_songs".to_string()));
        assert!(tables.contains(&"trash_items".to_string()));
        assert!(tables.contains(&"app_settings".to_string()));
        assert!(tables.contains(&"album_sources".to_string()));
        assert!(tables.contains(&"favorite_songs".to_string()));
    }

    #[tokio::test]
    async fn run_migrations_is_idempotent() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let migrations = get_migrations();

        run_migrations(&pool, &migrations).await.unwrap();
        // Second run must succeed and do nothing
        run_migrations(&pool, &migrations).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 9);
    }

    #[tokio::test]
    async fn run_migrations_tolerates_newer_migrations_without_error() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        // Pre-populate _sqlx_migrations with version 99 (simulating future release/rollback)
        sqlx::raw_sql(
            "CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL DEFAULT x'',
                execution_time BIGINT NOT NULL DEFAULT 0
            );
            INSERT INTO _sqlx_migrations (version, description, success) VALUES (99, 'future_migration', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Running current migrations should not panic or fail
        let migrations = get_migrations();
        run_migrations(&pool, &migrations).await.unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 10); // 9 from current + 1 future
    }

    #[tokio::test]
    async fn run_migrations_rolls_back_on_error() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        let bad_migrations = vec![
            Migration {
                version: 1,
                description: "valid step",
                sql: "CREATE TABLE valid_step (id INT);",
            },
            Migration {
                version: 2,
                description: "failing step",
                sql: "THIS IS INVALID SQL;",
            },
        ];

        let result = run_migrations(&pool, &bad_migrations).await;
        assert!(result.is_err());

        // Step 1 was committed before step 2 failed
        let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(applied, vec![1]);
    }
}
