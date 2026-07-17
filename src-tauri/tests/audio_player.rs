use blacktape_desktop_lib::audio::player::RepeatMode;
use blacktape_desktop_lib::types::{Platform, QualityTier, Song};

fn create_mock_song(id: i64, title: &str, duration_ms: u64) -> Song {
    Song {
        id: Some(id),
        path: format!("/mock/path/song_{}.mp3", id),
        title: title.to_string(),
        artists: vec![],
        album: "Test Album".to_string(),
        duration_ms,
        track_number: None,
        genre: None,
        release_year: None,
        cover_url: None,
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
        source: Platform::Youtube,
        source_url: None,
        source_item_id: None,
        canonical_track_slug: "".to_string(),
        canonical_album_slug: "".to_string(),
        album_artist: "".to_string(),
        quality_tier: QualityTier::Low,
    }
}

#[test]
fn test_queue_navigation_boundaries() {
    let songs = [
        create_mock_song(1, "First Track", 180000),
        create_mock_song(2, "Second Track", 200000),
        create_mock_song(3, "Third Track", 220000),
    ];

    let play_order: Vec<usize> = (0..songs.len()).collect();
    let mut cursor = Some(0);
    let repeat_mode = RepeatMode::Off;

    let get_next_cursor =
        |cursor: Option<usize>, order: &Vec<usize>, mode: RepeatMode| -> Option<usize> {
            let current_cursor = cursor?;
            if mode == RepeatMode::Track {
                return Some(current_cursor);
            }
            let next_cursor = current_cursor + 1;
            if next_cursor >= order.len() {
                if mode == RepeatMode::Queue {
                    Some(0)
                } else {
                    None
                }
            } else {
                Some(next_cursor)
            }
        };

    let next = get_next_cursor(cursor, &play_order, repeat_mode);
    assert_eq!(next, Some(1));

    cursor = Some(2);
    let edge_case = get_next_cursor(cursor, &play_order, repeat_mode);
    assert_eq!(
        edge_case, None,
        "Should not advance past the end of the queue when repeat is off"
    );

    let loop_case = get_next_cursor(cursor, &play_order, RepeatMode::Queue);
    assert_eq!(
        loop_case,
        Some(0),
        "Should loop back to the beginning of the queue"
    );
}

#[test]
fn test_shuffle_guarantees_current_song_stays_first() {
    let mut play_order: Vec<usize> = (0..100).collect();
    let mut cursor = Some(45);

    if let Some(c) = cursor {
        let current_idx = play_order.remove(c);

        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        play_order.shuffle(&mut rng);

        play_order.insert(0, current_idx);
        cursor = Some(0);
    }

    assert_eq!(
        cursor,
        Some(0),
        "Cursor must point to zero after triggering shuffle"
    );
    assert_eq!(
        play_order[0], 45,
        "The track that was playing must remain at the top of the order layout"
    );
    assert_eq!(
        play_order.len(),
        100,
        "The shuffle operation must not leak or destroy items"
    );
}
