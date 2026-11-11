use std::path::{Path, PathBuf};
use tauri_plugin_sql::{Migration, MigrationKind};

/// The app database as `tauri-plugin-sql` names it. It must match `plugins.sql.preload` in
/// `tauri.conf.json`: the plugin creates the file and runs the migrations at startup.
pub const DB_URL: &str = "sqlite:blacktape.db";

/// Where `tauri-plugin-sql` keeps `DB_URL`: a relative name is resolved against the app *config*
/// folder. The backend opens the same file through sqlx, so it must use this path too; the data
/// folder is the same on Windows and macOS but not on Linux.
pub fn db_file_path(app_config_dir: &Path) -> PathBuf {
    app_config_dir.join(DB_URL.trim_start_matches("sqlite:"))
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
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "add lyrics field",
            sql: "ALTER TABLE songs ADD COLUMN lyrics TEXT;",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 3,
            description: "add lyrics source field",
            sql: "ALTER TABLE songs ADD COLUMN lyrics_source TEXT;",
            kind: MigrationKind::Up,
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
            kind: MigrationKind::Up,
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
            kind: MigrationKind::Up,
        },
        Migration {
            version: 6,
            description: "create trash items and app settings tables",
            sql: TRASH_SCHEMA_SQL,
            kind: MigrationKind::Up,
        },
        Migration {
            version: 7,
            description: "create album sources table",
            sql: ALBUM_SOURCES_SCHEMA_SQL,
            kind: MigrationKind::Up,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_preloads_the_database_the_backend_opens() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let preload = &conf["plugins"]["sql"]["preload"];
        assert_eq!(preload, &serde_json::json!([DB_URL]));
    }

    #[test]
    fn database_file_sits_in_the_config_folder() {
        let dir = Path::new("config-dir");
        assert_eq!(db_file_path(dir), dir.join("blacktape.db"));
    }
}
