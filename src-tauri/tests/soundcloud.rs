//! Live SoundCloud checks (network). Run manually:
//! `cargo test --test soundcloud -- --ignored --nocapture`

use blacktape_desktop_lib::download::soundcloud::{parse_album_with_skipped, parse_track};
use blacktape_desktop_lib::soundcloud::{api_get, ScCollection, ScTrack};

#[tokio::test]
#[ignore]
async fn live_search_returns_tracks() {
    let page: ScCollection = api_get(
        "/search/tracks",
        &[("q", "daft punk".to_string()), ("limit", "10".to_string())],
    )
    .await
    .expect("search failed");
    let tracks: Vec<ScTrack> = page.items();
    assert!(!tracks.is_empty(), "no tracks for 'daft punk'");
    for t in &tracks {
        println!("{:?} {:?} {:?}", t.title, t.policy, t.permalink_url);
    }
}

#[tokio::test]
#[ignore]
async fn live_parse_track() {
    let (track, cover) = parse_track("https://soundcloud.com/forss/flickermood")
        .await
        .expect("parse_track failed");
    assert_eq!(track.title, "Flickermood");
    assert!(cover.unwrap_or_default().contains("t500x500"));
}

#[tokio::test]
#[ignore]
async fn live_parse_set_skips_previews() {
    let (album, skipped) =
        parse_album_with_skipped("https://soundcloud.com/the-concept-band/sets/the-royal-concept-ep")
            .await
            .expect("parse_album failed");
    println!("kept {} tracks, skipped {:?}", album.tracks.len(), skipped);
    assert!(!album.tracks.is_empty());
}
