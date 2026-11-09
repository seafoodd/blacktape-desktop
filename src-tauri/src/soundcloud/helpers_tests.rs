//! Unit tests for the pure URL, date and artwork helpers in the parent module.

use super::*;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/soundcloud/", $name))
    };
}

fn track() -> ScTrack {
    serde_json::from_str(fixture!("track_flickermood.json")).unwrap()
}

fn playlist() -> ScPlaylist {
    serde_json::from_str(fixture!("playlist_royal_concept_ep.json")).unwrap()
}

// --- upgrade_artwork_url ---------------------------------------------------------------

#[test]
fn artwork_large_is_upgraded_to_t500x500() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/artworks-000067273316-smsiqx-large.jpg"),
        "https://i1.sndcdn.com/artworks-000067273316-smsiqx-t500x500.jpg"
    );
}

#[test]
fn artwork_t300x300_is_upgraded_to_t500x500() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/artworks-abc-t300x300.jpg"),
        "https://i1.sndcdn.com/artworks-abc-t500x500.jpg"
    );
}

#[test]
fn artwork_crop_is_upgraded_to_t500x500() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/artworks-abc-crop.jpg"),
        "https://i1.sndcdn.com/artworks-abc-t500x500.jpg"
    );
}

#[test]
fn artwork_small_png_keeps_its_extension() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/artworks-abc-small.png"),
        "https://i1.sndcdn.com/artworks-abc-t500x500.png"
    );
}

#[test]
fn artwork_already_t500x500_is_unchanged() {
    let url = "https://i1.sndcdn.com/artworks-abc-t500x500.jpg";
    assert_eq!(upgrade_artwork_url(url), url);
}

#[test]
fn artwork_original_is_unchanged() {
    let url = "https://i1.sndcdn.com/artworks-abc-original.jpg";
    assert_eq!(upgrade_artwork_url(url), url);
}

#[test]
fn avatar_large_is_upgraded_to_t500x500() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/avatars-000012778523-59quu3-large.jpg"),
        "https://i1.sndcdn.com/avatars-000012778523-59quu3-t500x500.jpg"
    );
}

#[test]
fn url_without_size_token_is_unchanged() {
    let url = "https://i1.sndcdn.com/artworks-abc.jpg";
    assert_eq!(upgrade_artwork_url(url), url);
}

#[test]
fn query_string_after_extension_is_kept() {
    assert_eq!(
        upgrade_artwork_url("https://i1.sndcdn.com/artworks-abc-large.jpg?v=2"),
        "https://i1.sndcdn.com/artworks-abc-t500x500.jpg?v=2"
    );
}

// --- year_from_dates -------------------------------------------------------------------

#[test]
fn year_skips_none_and_takes_first_date() {
    assert_eq!(year_from_dates(&[None, Some("2012-07-28T17:51:00Z")]), Some(2012));
}

#[test]
fn year_skips_unparsable_date() {
    assert_eq!(
        year_from_dates(&[Some("garbage"), Some("2003-06-02T00:00:00Z")]),
        Some(2003)
    );
}

#[test]
fn year_is_none_for_empty_slice() {
    assert_eq!(year_from_dates(&[]), None);
}

#[test]
fn year_is_none_when_all_dates_are_none() {
    assert_eq!(year_from_dates(&[None, None]), None);
}

#[test]
fn year_skips_empty_string() {
    assert_eq!(year_from_dates(&[Some(""), Some("1999-01-01")]), Some(1999));
}

#[test]
fn year_skips_too_short_string() {
    assert_eq!(year_from_dates(&[Some("20"), Some("2010-05-05")]), Some(2010));
}

// --- next_offset_from_href -------------------------------------------------------------

#[test]
fn next_offset_is_read_from_query() {
    let href = "https://api-v2.soundcloud.com/search/tracks?query_urn=x&offset=20&q=a&limit=20";
    assert_eq!(next_offset_from_href(Some(href)), Some(20));
}

#[test]
fn next_offset_is_none_without_href() {
    assert_eq!(next_offset_from_href(None), None);
}

#[test]
fn next_offset_is_none_when_href_has_no_offset() {
    assert_eq!(
        next_offset_from_href(Some("https://api-v2.soundcloud.com/search/tracks?q=a")),
        None
    );
}

#[test]
fn next_offset_is_none_for_invalid_url() {
    assert_eq!(next_offset_from_href(Some("not a url")), None);
}

#[test]
fn next_offset_is_none_when_not_numeric() {
    assert_eq!(
        next_offset_from_href(Some("https://api-v2.soundcloud.com/search?offset=abc")),
        None
    );
}

// --- is_soundcloud_url -----------------------------------------------------------------

#[test]
fn soundcloud_track_url_is_recognised() {
    assert!(is_soundcloud_url("https://soundcloud.com/forss/flickermood"));
}

#[test]
fn soundcloud_mobile_host_is_recognised() {
    assert!(is_soundcloud_url("https://m.soundcloud.com/forss"));
}

#[test]
fn soundcloud_host_is_case_insensitive() {
    assert!(is_soundcloud_url("https://SoundCloud.com/x"));
}

#[test]
fn snd_sc_short_link_is_recognised() {
    assert!(is_soundcloud_url("https://snd.sc/abc"));
}

#[test]
fn lookalike_host_is_rejected() {
    assert!(!is_soundcloud_url("https://notsoundcloud.com/x"));
}

#[test]
fn soundcloud_as_subdomain_of_other_domain_is_rejected() {
    assert!(!is_soundcloud_url("https://soundcloud.com.evil.example/x"));
}

#[test]
fn other_platform_url_is_rejected() {
    assert!(!is_soundcloud_url("https://youtube.com/watch?v=1"));
}

#[test]
fn non_url_is_rejected() {
    assert!(!is_soundcloud_url("not a url"));
}

#[test]
fn empty_string_is_rejected() {
    assert!(!is_soundcloud_url(""));
}

// --- track_artwork ---------------------------------------------------------------------

#[test]
fn track_artwork_uses_upgraded_track_artwork() {
    assert_eq!(
        track_artwork(&track()).as_deref(),
        Some("https://i1.sndcdn.com/artworks-000067273316-smsiqx-t500x500.jpg")
    );
}

#[test]
fn track_artwork_falls_back_to_user_avatar() {
    let mut t = track();
    t.artwork_url = None;
    assert_eq!(
        track_artwork(&t).as_deref(),
        Some("https://i1.sndcdn.com/avatars-000012778523-59quu3-t500x500.jpg")
    );
}

#[test]
fn track_artwork_is_none_without_artwork_or_user() {
    let mut t = track();
    t.artwork_url = None;
    t.user = None;
    assert_eq!(track_artwork(&t), None);
}

// --- playlist_artwork ------------------------------------------------------------------

#[test]
fn playlist_artwork_uses_upgraded_playlist_artwork() {
    let p = playlist();
    assert_eq!(
        playlist_artwork(&p).as_deref(),
        Some("https://i1.sndcdn.com/artworks-000030896212-o16m9v-t500x500.jpg")
    );
}

#[test]
fn playlist_artwork_empty_string_falls_back_to_first_track_with_artwork() {
    let mut p = playlist();
    p.artwork_url = Some(String::new());
    assert_eq!(
        playlist_artwork(&p).as_deref(),
        Some("https://i1.sndcdn.com/artworks-acRKqXJcJGVN-0-t500x500.jpg")
    );
}

#[test]
fn playlist_artwork_falls_back_to_user_avatar_when_no_track_has_artwork() {
    let mut p = playlist();
    p.artwork_url = None;
    for t in p.tracks.iter_mut() {
        t.artwork_url = None;
    }
    assert_eq!(
        playlist_artwork(&p).as_deref(),
        Some("https://i1.sndcdn.com/avatars-000473822721-ujh21l-t500x500.jpg")
    );
}
