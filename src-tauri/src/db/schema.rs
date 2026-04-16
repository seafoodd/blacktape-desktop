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
                source_url TEXT,
                genre TEXT,
                release_year INTEGER,
                date_added DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
            kind: MigrationKind::Up,
        },
    ]
}