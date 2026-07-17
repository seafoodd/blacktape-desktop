use tauri_plugin_sql::{Migration, MigrationKind};

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
    ]
}
