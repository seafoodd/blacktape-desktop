//! Fixture-based tests. Track, set and user objects in `fixtures/soundcloud/` are real
//! api-v2 objects captured from public soundcloud.com pages (trimmed). The search
//! envelopes wrap those objects in api-v2's `collection` page shape. Client ids are fake.

use super::*;
use crate::search::bandcamp::ItemType;
use crate::search::soundcloud::{map_page, SearchKind};
use crate::types::Platform;

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

fn tc(preset: &str, protocol: &str, mime: &str, quality: Option<&str>, snipped: bool) -> ScTranscoding {
    ScTranscoding {
        url: format!("https://api-v2.soundcloud.com/media/soundcloud:tracks:1/x/stream/{protocol}"),
        preset: preset.into(),
        snipped,
        duration: Some(200_000),
        quality: quality.map(Into::into),
        format: ScFormat {
            protocol: protocol.into(),
            mime_type: mime.into(),
        },
    }
}

// --- client id -------------------------------------------------------------------------

#[test]
fn client_id_from_hydration_block() {
    let html = fixture!("home_with_hydration.html");
    assert_eq!(
        extract_client_id_from_hydration(html).as_deref(),
        Some("HydrationFakeClientId0123456789ab")
    );
    assert_eq!(extract_client_id_from_hydration(fixture!("home_without_hydration.html")), None);
}

#[test]
fn bundle_urls_in_page_order_whatever_the_attribute_order() {
    let urls = extract_bundle_urls(fixture!("home_without_hydration.html"));
    assert_eq!(urls.len(), 9);
    assert_eq!(urls[0], "https://a-v2.sndcdn.com/assets/59-ac0a49ce.js");
    // `<script src="…" crossorigin>` (attribute order swapped) is found too.
    assert_eq!(urls[2], "https://a-v2.sndcdn.com/assets/18-2ab69eb1.js");
    assert_eq!(urls[8], "https://a-v2.sndcdn.com/assets/55-9bfd359c.js");
    // Stylesheets and images under /assets/ are not bundles.
    assert!(urls.iter().all(|u| u.ends_with(".js")));
}

#[test]
fn client_id_from_bundle() {
    assert_eq!(
        extract_client_id_from_bundle(fixture!("app_bundle_sample.js")).as_deref(),
        Some("BundleFakeClientId0123456789abcd")
    );
    assert_eq!(extract_client_id_from_bundle(fixture!("vendor_bundle_sample.js")), None);
}

#[test]
fn client_id_from_bundle_query_string_form() {
    let js = r#"fetch("https://api-v2.soundcloud.com/me?client_id=QueryFakeClientId0123456789abc&app_version=1")"#;
    assert_eq!(
        extract_client_id_from_bundle(js).as_deref(),
        Some("QueryFakeClientId0123456789abc")
    );
    // Too short to be an id (e.g. a variable placeholder).
    assert_eq!(extract_client_id_from_bundle(r#"client_id:"abc""#), None);
}

#[test]
fn api_url_adds_client_id_and_replaces_stale_params() {
    let url = build_api_url(
        "https://api-v2.soundcloud.com/search/tracks?q=old&offset=20&client_id=stale&query_urn=x",
        &[("q", "daft punk".into()), ("limit", "20".into())],
        "fresh",
    )
    .unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("offset".into(), "20".into()),
            ("query_urn".into(), "x".into()),
            ("q".into(), "daft punk".into()),
            ("limit".into(), "20".into()),
            ("client_id".into(), "fresh".into()),
        ]
    );
    let resolve = build_api_url(
        "https://api-v2.soundcloud.com/resolve",
        &[("url", "https://soundcloud.com/forss/flickermood?in=a/sets/b".into())],
        "id",
    )
    .unwrap();
    assert_eq!(
        resolve.as_str(),
        "https://api-v2.soundcloud.com/resolve?url=https%3A%2F%2Fsoundcloud.com%2Fforss%2Fflickermood%3Fin%3Da%2Fsets%2Fb&client_id=id"
    );
}

// --- transcoding choice ----------------------------------------------------------------

#[test]
fn hls_only_track_picks_aac_160() {
    let t = track();
    let chosen = choose_transcoding(&t.media.as_ref().unwrap().transcodings).unwrap();
    assert_eq!(chosen.preset, "aac_160k");
    assert_eq!(chosen.format.protocol, "hls");
}

#[test]
fn progressive_mp3_beats_hls() {
    let list = vec![
        tc("aac_160k", "hls", "audio/mp4; codecs=\"mp4a.40.2\"", Some("sq"), false),
        tc("mp3_1_0", "progressive", "audio/mpeg", Some("sq"), false),
        tc("opus_0_0", "hls", "audio/ogg; codecs=\"opus\"", Some("sq"), false),
    ];
    assert_eq!(choose_transcoding(&list).unwrap().preset, "mp3_1_0");
}

#[test]
fn snipped_preview_and_encrypted_streams_are_never_chosen() {
    let mut preview_url = tc("mp3_1_0", "progressive", "audio/mpeg", Some("sq"), false);
    preview_url.url = "https://api-v2.soundcloud.com/media/soundcloud:tracks:1/x/preview/progressive".into();
    let list = vec![
        tc("mp3_0_0", "progressive", "audio/mpeg", Some("sq"), true),
        preview_url,
        tc("aac_256k", "ctr-encrypted-hls", "audio/mp4", Some("hq"), false),
        tc("aac_96k", "hls", "audio/mp4", Some("lq"), false),
    ];
    assert_eq!(choose_transcoding(&list).unwrap().preset, "aac_96k");
    assert!(choose_transcoding(&list[..3]).is_none());
}

#[test]
fn adaptive_master_playlist_is_only_a_fallback() {
    let list = vec![
        tc("abr_sq", "hls", "audio/mpegurl", Some("sq"), false),
        tc("aac_96k", "hls", "audio/mp4", Some("lq"), false),
    ];
    assert_eq!(choose_transcoding(&list).unwrap().preset, "aac_96k");
    assert_eq!(choose_transcoding(&list[..1]).unwrap().preset, "abr_sq");
}

// --- availability / preview detection --------------------------------------------------

#[test]
fn full_track_is_available() {
    assert_eq!(availability(&track()), Availability::Full);
}

#[test]
fn go_plus_snip_track_is_preview_only() {
    let p = playlist();
    let snipped = &p.tracks[0];
    assert_eq!(snipped.policy.as_deref(), Some("SNIP"));
    assert_eq!(availability(snipped), Availability::PreviewOnly);
    assert!(Availability::PreviewOnly.reason().unwrap().contains("Go+"));
}

#[test]
fn snipped_streams_without_snip_policy_are_still_preview_only() {
    let mut t = playlist().tracks[0].clone();
    t.policy = Some("ALLOW".into());
    assert_eq!(availability(&t), Availability::PreviewOnly);
}

#[test]
fn thirty_second_duration_on_long_track_is_preview_only() {
    let mut t = track();
    t.duration = Some(30_000);
    t.full_duration = Some(213_890);
    assert_eq!(availability(&t), Availability::PreviewOnly);
    // A genuinely short track is fine.
    t.full_duration = Some(30_000);
    assert_eq!(availability(&t), Availability::Full);
}

#[test]
fn geo_blocked_and_unstreamable_tracks() {
    let mut t = track();
    t.policy = Some("BLOCK".into());
    assert_eq!(availability(&t), Availability::GeoBlocked);

    let mut t = track();
    t.streamable = Some(false);
    assert!(matches!(availability(&t), Availability::Unavailable(_)));

    let mut t = track();
    t.media = Some(ScMedia::default());
    assert!(matches!(availability(&t), Availability::Unavailable(_)));
}

#[test]
fn stub_track_is_unavailable_until_hydrated() {
    let p = playlist();
    let stub = &p.tracks[5];
    assert!(stub.is_stub());
    assert!(matches!(availability(stub), Availability::Unavailable(_)));
}

#[test]
fn merge_hydrated_tracks_keeps_set_order() {
    let mut p = playlist();
    let mut full = track();
    full.id = p.tracks[5].id;
    let mut unrelated = track();
    unrelated.id = 1;
    merge_hydrated_tracks(&mut p.tracks, vec![unrelated, full]);
    assert_eq!(p.tracks.len(), 6);
    assert!(!p.tracks[5].is_stub());
    assert_eq!(p.tracks[5].id, 47127631);
    assert_eq!(p.tracks[1].title.as_deref(), Some("The Royal Concept - Gimme Twice"));
}

// --- artist / title --------------------------------------------------------------------

#[test]
fn artist_prefix_matching_uploader_is_stripped() {
    let t = &playlist().tracks[1];
    assert_eq!(
        artist_and_title(t),
        ("The Royal Concept".to_string(), "Gimme Twice".to_string())
    );
}

#[test]
fn publisher_artist_wins_and_unrelated_dash_is_kept() {
    let mut t = track();
    t.title = Some("Flickermood - Live Edit".into());
    assert_eq!(
        artist_and_title(&t),
        ("Forss".to_string(), "Flickermood - Live Edit".to_string())
    );

    let mut t = track();
    t.publisher_metadata = Some(ScPublisherMetadata {
        artist: Some("Eric Wahlforss".into()),
        ..Default::default()
    });
    t.title = Some("Eric Wahlforss – Flickermood".into());
    assert_eq!(
        artist_and_title(&t),
        ("Eric Wahlforss".to_string(), "Flickermood".to_string())
    );
}

#[test]
fn title_that_is_only_the_prefix_is_not_emptied() {
    let mut t = track();
    t.title = Some("Forss - ".into());
    let (_, title) = artist_and_title(&t);
    assert!(!title.is_empty());
}

#[test]
fn secret_token_is_appended_to_download_url() {
    let mut t = track();
    assert_eq!(
        track_download_url(&t).as_deref(),
        Some("https://soundcloud.com/forss/flickermood")
    );
    t.secret_token = Some("s-AbCdE".into());
    assert_eq!(
        track_download_url(&t).as_deref(),
        Some("https://soundcloud.com/forss/flickermood/s-AbCdE")
    );
}

// --- search parsing --------------------------------------------------------------------

#[test]
fn search_tracks_page_drops_previews() {
    let page: ScCollection = serde_json::from_str(fixture!("search_tracks.json")).unwrap();
    let mapped = map_page(&page, SearchKind::Tracks);
    // Third item is a Go+ preview and must not be offered.
    let names: Vec<&str> = mapped.items.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Flickermood", "Gimme Twice"]);
    assert_eq!(mapped.next_offset, Some(20));
    assert_eq!(mapped.total_results, Some(1234));

    let first = &mapped.items[0];
    assert_eq!(first.item_type, ItemType::Track);
    assert_eq!(first.platform, Platform::Soundcloud);
    assert_eq!(first.band_name, "Forss");
    assert_eq!(first.item_url_path, "https://soundcloud.com/forss/flickermood");
    assert_eq!(first.img, "https://i1.sndcdn.com/artworks-000067273316-smsiqx-t500x500.jpg");
    assert_eq!(first.duration, Some(213));
    assert_eq!(first.year, Some(2003));
    assert!(first.view_count.unwrap_or(0) > 0);
}

#[test]
fn search_playlists_page_maps_sets_to_albums() {
    let page: ScCollection = serde_json::from_str(fixture!("search_playlists.json")).unwrap();
    let mapped = map_page(&page, SearchKind::Sets);
    assert_eq!(mapped.items.len(), 1);
    let set = &mapped.items[0];
    assert_eq!(set.item_type, ItemType::Album);
    assert_eq!(set.name, "The Royal Concept EP");
    assert_eq!(set.band_name, "The Royal Concept");
    assert_eq!(
        set.item_url_path,
        "https://soundcloud.com/the-concept-band/sets/the-royal-concept-ep"
    );
    assert_eq!(set.year, Some(2012));
    assert!(set.img.ends_with("-t500x500.jpg"));
    assert_eq!(mapped.next_offset, None);
}

#[test]
fn search_users_page_maps_artists() {
    let page: ScCollection = serde_json::from_str(fixture!("search_users.json")).unwrap();
    let mapped = map_page(&page, SearchKind::Users);
    assert_eq!(mapped.items.len(), 1);
    let user = &mapped.items[0];
    assert_eq!(user.item_type, ItemType::Artist);
    assert_eq!(user.name, "Forss");
    assert_eq!(user.subscriber_count, Some(132108));
    assert_eq!(user.img, "https://i1.sndcdn.com/avatars-000012778523-59quu3-t500x500.jpg");
}

#[test]
fn malformed_search_item_does_not_fail_the_page() {
    let json = format!(
        r#"{{"collection":[{{"oops":true}}, {}],"next_href":null}}"#,
        fixture!("track_flickermood.json")
    );
    let page: ScCollection = serde_json::from_str(&json).unwrap();
    assert_eq!(map_page(&page, SearchKind::Tracks).items.len(), 1);
}

#[test]
fn resolve_response_is_discriminated_by_kind() {
    let r: ScResource = serde_json::from_str(fixture!("track_flickermood.json")).unwrap();
    assert!(matches!(r, ScResource::Track(t) if t.id == 293));
    let r: ScResource = serde_json::from_str(fixture!("playlist_royal_concept_ep.json")).unwrap();
    assert!(matches!(r, ScResource::Playlist(p) if p.tracks.len() == 6));
    let r: ScResource = serde_json::from_str(fixture!("user_forss.json")).unwrap();
    assert!(matches!(r, ScResource::User(u) if u.username == "Forss"));
    let r: ScResource = serde_json::from_str(r#"{"kind":"system-playlist","id":"x"}"#).unwrap();
    assert!(matches!(r, ScResource::Other));
}
