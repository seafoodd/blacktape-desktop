use blacktape_desktop_lib::db::db::Database;
use blacktape_desktop_lib::types::Song;

async fn setup_test_db() -> Database {
    let db = Database::new(":memory:").await;

    sqlx::query(
        "CREATE TABLE songs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT NOT NULL,
            track_number INTEGER,
            duration_ms INTEGER NOT NULL,
            cover_url TEXT,
            external_cover_url TEXT,
            genre TEXT,
            release_year INTEGER,
            lyrics TEXT,
            lyrics_source TEXT
        );",
    )
    .execute(&db.pool)
    .await
    .unwrap();

    db
}

#[tokio::test]
async fn test_song_insertion_and_mapping() {
    let db = setup_test_db().await;

    let mock_song = Song {
        id: None,
        path: "/music/track1.mp3".to_string(),
        title: "Test Track".to_string(),
        artist: "The Testers".to_string(),
        album: "Beta Album".to_string(),
        track_number: Some(1),
        duration_ms: 180_000,
        cover_url: None,
        external_cover_url: None,
        genre: None,
        release_year: Some(2026),
        lyrics: None,
        lyrics_source: None,
    };

    let insert_res = db.insert_song(mock_song.clone()).await;
    assert!(
        insert_res.is_ok(),
        "Insertion crashed: {:?}",
        insert_res.err()
    );

    let all_songs = db.get_all_songs().await.unwrap();
    assert_eq!(all_songs.len(), 1);
    assert_eq!(all_songs[0].title, "Test Track");
    assert_eq!(all_songs[0].duration_ms, 180_000);
}

#[tokio::test]
async fn test_artist_summary_aggregation() {
    let db = setup_test_db().await;

    let song_a = Song {
        id: None,
        path: "a.mp3".into(),
        title: "Bohemian Rhapsody".into(),
        artist: "Queen".into(),
        album: "A Night at the Opera".into(),
        track_number: Some(1),
        duration_ms: 355_000,
        cover_url: None,
        external_cover_url: None,
        genre: None,
        release_year: Some(1795),
        lyrics: None,
        lyrics_source: None,
    };

    let song_b = Song {
        id: None,
        path: "b.mp3".into(),
        title: "We Will Rock You".into(),
        artist: "Queen".into(),
        album: "News of the World".into(),
        track_number: Some(1),
        duration_ms: 122_000,
        cover_url: None,
        external_cover_url: None,
        genre: None,
        release_year: Some(1977),
        lyrics: None,
        lyrics_source: None,
    };

    db.insert_song(song_a).await.unwrap();
    db.insert_song(song_b).await.unwrap();

    let summaries = db.get_artists_summary().await.unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].name, "Queen");
    assert_eq!(summaries[0].album_count, 2, "COUNT(DISTINCT album) failed!");
}
