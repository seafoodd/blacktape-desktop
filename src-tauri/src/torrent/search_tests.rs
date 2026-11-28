use super::*;
use serde_json::json;

const HASH: &str = "1111111111111111111111111111111111111111";
const OTHER_HASH: &str = "2222222222222222222222222222222222222222";

#[tokio::test]
#[ignore = "live RuTor search and metadata; upstream releases can change"]
async fn live_rutor_search_and_inspection() {
    for (artist, title, tracks) in [
        ("Aphex Twin", "Syro", 12),
        ("The Doors", "The Soft Parade", 25),
        ("glass beach", "plastic death", 13),
    ] {
        let report = search_for_release(artist, title, true, &[Provider::Rutor], |_| {}).await.unwrap();
        assert!(report.providers[0].error.is_none(), "{title}: {:?}", report.providers[0].error);
        let candidate = report.hits.iter().find(|hit| hit.title.to_ascii_lowercase().contains("flac"))
            .unwrap_or_else(|| panic!("{title}: no FLAC candidate"));
        let release = crate::torrent::inspect::inspect(&candidate.input).await.unwrap();
        assert_eq!(release.tracks.iter().filter(|track| track.codec == "FLAC" && !track.is_sample).count(), tracks, "{title}");
        println!("{title}: verified {tracks} FLAC tracks");
    }
}

#[test]
fn rutracker_matches_genre_prefixed_releases_and_rejects_wrong_artists_and_urls() {
    let rows = serde_json::from_value(json!([
        {"title":"(Alternative Rock) [FLAC] Radiohead - OK Computer (1997)", "link":"dl.php?t=42", "seeders":8},
        {"title":"Radiohead - OK Computer [FLAC]", "link":"//evil.test/forum/dl.php?t=43", "seeders":9},
        {"title":"Different Artist - OK Computer [FLAC]", "link":"dl.php?t=44", "seeders":9}
    ])).unwrap();
    let hits = rutracker_hits(&Query::new("Radiohead", "OK Computer").unwrap(), rows);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].input, "rutracker:42");
    assert_eq!(hits[0].seeders, Some(8));
    assert_eq!(hits[0].info_hash, None);
}

#[tokio::test]
#[ignore = "live SolidTorrents benchmark; swarm availability changes"]
async fn live_solid_torrents_benchmark() {
    use librqbit::{AddTorrent, AddTorrentOptions, Session, SessionOptions};

    let directory = tempfile::tempdir().unwrap();
    let session = Session::new_with_opts(
        directory.path().to_path_buf(),
        SessionOptions {
            disable_dht_persistence: true,
            disable_upload: true,
            ..Default::default()
        },
    ).await.unwrap();
    let mut jobs = JoinSet::new();
    for (artist, title, hash) in [
        ("Chris Christodoulou", "DEADBOLT", "5EBF808DA59CFAF0DCFEF42558F47E6EB831BA33"),
        ("glass beach", "plastic death", "A5D12F556A8BE0CDD0F7645D27E4ACF6EB8D20CC"),
        ("glass beach", "the first glass beach album", "763A9989CF157EB2F2B91FA4BBBEB3878A91F346"),
        ("Car Seat Headrest", "Making a Door Less Open", "BCB814A0DB7679961B864487E02DDD8BB4EE8765"),
        ("Car Seat Headrest", "Teens of Denial", "33A692A223C084B317A461EE9D6FEE97E843C3D2"),
    ] {
        let session = session.clone();
        jobs.spawn(async move {
            let baseline = search_for_release(artist, title, true, &[Provider::Archive, Provider::Knaben, Provider::ApiBay], |_| {}).await.unwrap();
            println!("{title}: baseline duplicate={}, baseline hits={}, errors={:?}",
                baseline.hits.iter().any(|hit| hit.info_hash.as_ref().is_some_and(|id| id.eq_ignore_ascii_case(hash))),
                baseline.hits.len(), baseline.providers.iter().filter_map(|p| p.error.as_ref()).collect::<Vec<_>>());
            let solid = search_for_release(artist, title, true, &[Provider::SolidTorrents], |_| {}).await.unwrap();
            assert!(solid.providers[0].error.is_none(), "{title}: {:?}", solid.providers[0].error);
            let Some(hit) = solid.hits.iter().find(|hit| hit.info_hash.as_ref().is_some_and(|id| id.eq_ignore_ascii_case(hash))) else {
                println!("{title}: previous candidate was absent from the live results");
                return;
            };
            let uri = &hit.input;
            let release = match crate::torrent::inspect::inspect(uri).await {
                Ok(release) => release,
                Err(error) => {
                    println!("{title}: inspection failed: {error}");
                    return;
                }
            };
            let flac = release.tracks.iter().filter(|track| track.codec == "FLAC" && !track.is_sample).collect::<Vec<_>>();
            println!("{title}: Blacktape metadata confirms {} FLAC tracks", flac.len());
            let Some(track) = flac.iter().min_by_key(|track| track.size_bytes) else { return; };
            let source = crate::torrent::inspect::cached_torrent_bytes(uri)
                .map(AddTorrent::from_bytes)
                .unwrap_or_else(|| AddTorrent::from_url(uri.as_str()));
            let transfer = tokio::time::timeout(Duration::from_secs(30), async {
                let handle = session.add_torrent(source, Some(AddTorrentOptions {
                    only_files: Some(vec![track.file_index]),
                    ..Default::default()
                })).await.map_err(|error| error.to_string())?.into_handle().ok_or("Missing torrent handle".to_string())?;
                loop {
                    let stats = handle.stats();
                    if stats.progress_bytes > 0 {
                        return Ok::<_, String>(stats.progress_bytes);
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }).await;
            println!("{title}: transfer={transfer:?}");
        });
    }
    while let Some(result) = jobs.join_next().await {
        result.unwrap();
    }
    session.stop().await;
}

#[test]
fn solid_torrents_search_keeps_misclassified_audio_but_rejects_video_and_bad_hashes() {
    let query = Query::new("A+B", "One & Two").unwrap();
    let client = Client::new();
    let request = providers::solid_torrents::request(&client, &query, false).build().unwrap();
    assert_eq!(request.url().host_str(), Some("solidtorrents.eu"));
    let parameters: BTreeMap<_, _> = request.url().query_pairs().into_owned().collect();
    assert_eq!(parameters.get("q").map(String::as_str), Some("A+B One & Two"));
    assert_eq!(parameters.get("limit").map(String::as_str), Some("30"));
    assert!(!parameters.contains_key("category"));

    let rows = json!({"success": true, "results": [
        {"title": "A+B - One & Two [FLAC]", "infohash": HASH, "category": 1, "subCategory": 3,
         "seeders": 4, "size": 123, "magnetUri": "magnet:?tr=http://127.0.0.1"},
        {"title": "A+B - One & Two [MP3]", "infohash": OTHER_HASH, "category": 7, "subCategory": 1},
        {"title": "A+B - One & Two", "infohash": "3333333333333333333333333333333333333333", "category": 7, "subCategory": 4},
        {"title": "A+B - One & Two", "infohash": "4444444444444444444444444444444444444444", "category": 2},
        {"title": "Different Artist - One & Two", "infohash": "5555555555555555555555555555555555555555", "category": 7},
        {"title": "A+B - One & Two", "infohash": "invalid", "category": 7},
    ]});
    let hits = providers::solid_torrents::parse(&rows, &query).unwrap();
    assert_eq!(hits.len(), 2);
    let flac = hits.iter().find(|hit| hit.info_hash.as_deref() == Some(HASH)).unwrap();
    assert_eq!(flac.provider, "SolidTorrents");
    assert_eq!(flac.seeders, Some(4));
    assert_eq!(flac.size_bytes, Some(123));
    assert_eq!(flac.input, format!("magnet:?xt=urn:btih:{HASH}"));
}

#[test]
fn solid_torrents_distinguishes_empty_queries_from_upstream_errors() {
    let query = Query::new("Artist", "Album").unwrap();
    let parse = |root| providers::solid_torrents::parse(&root, &query);
    assert!(parse(json!({"success": true, "results": []})).unwrap().is_empty());
    assert!(matches!(parse(json!({"success": false, "results": []})), Err(ProviderError::Unavailable)));
    for root in [json!({"success": true}), json!({"success": true, "results": [{}]}), json!({"results": []})] {
        assert!(matches!(parse(root), Err(ProviderError::Malformed)));
    }
}

#[tokio::test]
#[ignore = "live indexer connection checks"]
async fn live_indexer_settings_connection_checks() {
    for id in Provider::ALL {
        let result = test_provider(id, None).await.unwrap();
        assert_eq!(result.provider, id.name());
        println!("{}: {} sample results, {:?}", result.provider, result.hits.len(), result.error);
    }
}

#[test]
fn structured_identity_distinguishes_numeric_titles_artists_and_versions() {
    for (artist, title, candidate, matches) in [
        ("The Beatles", "1", "The Beatles - 1 [FLAC]", true),
        ("The Beatles", "1", "The Beatles - 1967–1970", false),
        ("The Beatles", "1", "The Beatles - 1 Love", false),
        ("X", "Los Angeles", "X - Los Angeles (1980) [FLAC]", true),
        ("X", "Los Angeles", "X Japan - Los Angeles", false),
        ("Björk", "Vespertine", "bjork.vespertine.2001.flac", true),
        ("宇多田ヒカル", "光", "宇多田ヒカル - 光 [FLAC]", true),
        ("Artist", "Song", "Artist - Song (Live)", false),
        ("Artist", "Song (Live)", "Artist - Song (Live) [FLAC]", true),
        ("Artist", "Song (Live)", "Artist - Song [FLAC]", false),
        ("Artist", "Song", "Artist - Song (Champion Remix)", false),
        (
            "Black Rebel Motorcycle Club",
            "B.R.M.C.",
            "Black Rebel Motorcycle Club - B.R.M.C. [2001] [FLAC]",
            true,
        ),
    ] {
        let query = Query::new(artist, title).unwrap();
        assert_eq!(
            query.identity(candidate, &[]).is_some(),
            matches,
            "{artist} / {title} against {candidate}"
        );
    }
}

#[test]
fn real_album_anniversary_titles_match_without_accepting_unrelated_versions() {
    let doors = Query::new("The Doors", "The Soft Parade").unwrap();
    assert_eq!(doors.identity("The Doors - The Soft Parade [50th Anniversary Deluxe] (2019) [FLAC 24-192]", &[]), Some(2));
    assert_eq!(doors.identity("The Doors - The Soft Parade (50th Ann Deluxe Edition) [2019]", &[]), Some(2));
    assert_eq!(doors.identity("The Doors - The Soft Parade 50th live", &[]), None);
    assert_eq!(doors.identity("The Doors - The Soft Parade anniversary concert", &[]), None);
    let radiohead = Query::new("Radiohead", "OK Computer").unwrap();
    assert_eq!(radiohead.identity("Radiohead - OK Computer (20th Anniversary Edition) [FLAC]", &[]), Some(2));
    assert_eq!(radiohead.identity("Radiohead - OK Computer 2nd album live", &[]), None);
}

#[test]
fn recognizes_full_album_labels_and_the_reversed_ep_title() {
    let glass_beach = Query::new("glass beach", "the first glass beach album").unwrap();
    assert_eq!(glass_beach.identity("glass beach - the first glass beach album (full album)", &[]), Some(3));
    for variant in ["demos", "stems", "full album instrumental"] {
        assert_eq!(glass_beach.identity(&format!("glass beach - the first glass beach album ({variant})"), &[]), None);
    }
    let ep = Query::new("Car Seat Headrest", "Living While Starving").unwrap();
    assert_eq!(ep.text(false), "Car Seat Headrest Starving While Living");
    assert_eq!(ep.identity("Car Seat Headrest - Starving While Living [FLAC]", &[]), Some(3));
    assert_eq!(Query::new("Another Artist", "Living While Starving").unwrap().title, "Living While Starving");
}

#[test]
fn distinguishes_soundtrack_labels_from_candidates_that_need_a_tracklist() {
    let mut query = Query::new("Chris Christodoulou", "DEADBOLT").unwrap();
    for name in [
        "Chris Christodoulou - DEADBOLT (Original Game Soundtrack) [FLAC]",
        "DEADBOLT OST - Chris Christodoulou (2016)",
        "[2016] DEADBOLT OST - Chris Christodoulou",
    ] {
        assert_eq!(query.identity(name, &[]), Some(2), "{name}");
    }
    assert_eq!(query.identity("DEADBOLT OST", &[]), None);
    query.verify_tracklist = true;
    assert_eq!(query.identity("DEADBOLT OST", &[]), Some(1));
    assert_eq!(query.identity("Chris Christodoulou - DEADBOLT [BSR press]", &[]), Some(1));
    for (name, creators) in [
        ("DEADBOLT OST", &["Deadbolt"][..]),
        ("DEADBOLT Discography", &[]),
        ("Chris Christodoulou - DEADBOLT Cross-Section Stems", &[]),
        ("Chris Christodoulou - DEADBOLT OST Live", &[]),
        ("Deadbolt Zen - Weightless", &[]),
        ("DEADBOLT 2 [FLAC]", &[]),
    ] {
        assert_eq!(query.identity(name, creators), None, "{name}");
    }
    let mut numeric = Query::new("Car Seat Headrest", "4").unwrap();
    numeric.verify_tracklist = true;
    assert!(!numeric.can_search_title_only());
    assert_eq!(numeric.identity("4 [FLAC]", &[]), None);
}

#[tokio::test]
#[ignore = "live provider diagnostic; results and availability change"]
async fn live_requested_albums_and_comparisons() {
    for (artist, title) in [
        ("The Doors", "The Soft Parade"),
        ("Aphex Twin", "drukqs"),
        ("Radiohead", "OK Computer"),
        ("Nirvana", "Nevermind"),
    ] {
        let report = search(artist, title, |_| {}).await.unwrap();
        let providers: Vec<_> = report.providers.iter().map(|p| (&p.provider, p.hits.len(), &p.error)).collect();
        let hits: Vec<_> = report.hits.iter().take(5).map(|h| (&h.title, &h.provider, h.seeders)).collect();
        println!("{artist} / {title}: providers={providers:?}, hits={hits:?}");
        assert!(!report.hits.is_empty(), "no verified provider results for {artist} / {title}");
    }
}

#[tokio::test]
#[ignore = "live provider and metadata diagnostic; availability changes"]
async fn live_search_inspection_and_cached_metadata() {
    for (artist, title, track) in [
        ("The Doors", "The Soft Parade", "touch me"),
        ("Aphex Twin", "drukqs", "vordhosbn"),
        ("Radiohead", "OK Computer", "paranoid android"),
        ("Nirvana", "Nevermind", "smells like teen spirit"),
    ] {
        let report = search(artist, title, |_| {}).await.unwrap();
        let mut found = false;
        for hit in report.hits.iter().filter(|h| h.seeders.is_some_and(|n| n > 0)).take(3) {
            match crate::torrent::inspect::inspect(&hit.input).await {
                Ok(release) => {
                    let matches = release.tracks.iter().any(|file| normalize(&file.title).contains(track));
                    let cached = crate::torrent::inspect::cached_torrent_bytes(&hit.input).is_some();
                    println!("{artist} / {title}: {} peers={:?} track={matches} cached={cached}", hit.provider, hit.seeders);
                    if matches && cached { found = true; break; }
                }
                Err(error) => println!("{artist} / {title}: inspection failed: {error}"),
            }
        }
        assert!(found, "no inspected seeded result with {track} for {artist} / {title}");
    }
}

#[test]
fn archive_search_escapes_lucene_and_http_parameters_once() {
    let query = Query::new("A+B", "One (Live) & Two").unwrap();
    let client = reqwest::Client::new();
    let request = providers::archive::search_request(&client, &query, false)
        .build()
        .unwrap();
    let parameters = request.url().query_pairs().collect::<Vec<_>>();
    assert!(parameters.iter().any(|(key, value)| {
        key == "q"
            && value == r#"mediatype:audio AND ((creator:"A\+B" AND title:"One \(Live\) & Two") OR (title:"A\+B" AND title:"One \(Live\) & Two"))"#
    }));
    assert!(parameters
        .iter()
        .any(|(key, value)| key == "fl[]" && value == "identifier"));
    let fallback = providers::archive::search_request(&client, &query, true).build().unwrap();
    assert!(fallback.url().query_pairs().any(|(key, value)| {
        key == "q" && value == r#"mediatype:audio AND title:"One \(Live\) & Two""#
    }));
}

#[tokio::test]
#[ignore = "live catalog and metadata coverage depend on public providers"]
async fn live_requested_artist_catalog_coverage() {
    let catalog: &[(&str, &[&str], &[&str])] = &[
        ("Aphex Twin", &["Selected Ambient Works 85-92", "Selected Ambient Works Volume II", "I Care Because You Do", "Richard D. James Album", "drukqs", "Syro", "Classics"], &["Xtal", "4", "Vordhosbn", "Avril 14th"]),
        ("The Doors", &["The Doors", "Strange Days", "Waiting for the Sun", "The Soft Parade", "Morrison Hotel", "L.A. Woman", "Other Voices", "Full Circle", "An American Prayer"], &["Light My Fire", "Touch Me", "The End"]),
        ("The Beatles", &["Please Please Me", "With the Beatles", "A Hard Day’s Night", "Beatles for Sale", "Help!", "Rubber Soul", "Revolver", "Sgt. Pepper’s Lonely Hearts Club Band", "Magical Mystery Tour", "The Beatles", "Yellow Submarine", "Abbey Road", "Let It Be", "Past Masters", "1"], &["Yesterday", "Hey Jude", "The End"]),
        ("glass beach", &["the first glass beach album", "plastic death"], &["classic j dies and goes to hell part 1", "bedroom community"]),
        ("Car Seat Headrest", &["Twin Fantasy", "Teens of Style", "Teens of Denial", "Monomania", "Nervous Young Man", "How to Leave Town", "Making a Door Less Open"], &["Beach Life-in-Death", "Bodys", "Drunk Drivers/Killer Whales"]),
        ("Sewerslvt", &["Drowning In The Sewer", "Draining Love Story", "The World Is Fvcked", "Skitzofrenia Simulation", "we had good times together, don't forget that"], &["Mr. Kill Myself", "MAKE-ME-SAD", "I Really Like You pt2"]),
    ];
    let mut summary = [0; 4];
    let cases = catalog.iter().flat_map(|&(artist, albums, songs)| {
        albums.iter().map(move |&title| (artist, title, true))
            .chain(songs.iter().map(move |&title| (artist, title, false)))
    }).collect::<Vec<_>>();
    for batch in cases.chunks(4) {
        let mut jobs = JoinSet::new();
        for &(artist, title, is_album) in batch {
            jobs.spawn(async move {
            let report = search(artist, title, |_| {}).await.unwrap();
            let seeded = report.hits.iter().filter(|hit| hit.seeders.is_some_and(|n| n > 0)).count();
            let mut verified = 0;
            let mut seeded_lossless = 0;
            let mut hits: Vec<_> = report.hits.iter().filter(|hit| hit.seeders.is_some_and(|n| n > 0) || hit.provider == "Archive.org").collect();
            hits.sort_by_key(|hit| !(normalize(&hit.title).split_whitespace().any(|word| matches!(word, "flac" | "alac" | "wav" | "lossless"))));
            for hit in hits.into_iter().take(2) {
                let inspection = tokio::time::timeout(Duration::from_secs(12), crate::torrent::inspect::inspect(&hit.input)).await;
                let Ok(Ok(release)) = inspection else { continue };
                let lossless = |track: &crate::torrent::release::InspectedTrack| {
                    !track.is_sample && matches!(track.codec, "FLAC" | "ALAC" | "WAV" | "AIFF" | "APE" | "WAVPACK")
                };
                if (is_album && release.tracks.iter().filter(|track| lossless(track)).count() >= 2)
                    || (!is_album && release.tracks.iter().any(|track| lossless(track) && normalize(&track.title) == normalize(title))) {
                    verified += 1;
                    seeded_lossless += usize::from(hit.seeders.is_some_and(|n| n > 0));
                }
            }
            println!("{artist} / {title}: hits={}, seeded={seeded}, lossless_metadata={verified}, seeded_lossless={seeded_lossless}", report.hits.len());
            [1, usize::from(seeded > 0), usize::from(verified > 0), usize::from(seeded_lossless > 0)]
            });
        }
        while let Some(result) = jobs.join_next().await {
            for (total, count) in summary.iter_mut().zip(result.unwrap()) {
                *total += count;
            }
        }
    }
    println!("CATALOG: {} cases, {} with reported seeders, {} with lossless file metadata, {} with both on the same release", summary[0], summary[1], summary[2], summary[3]);
    assert!(summary[2] > 0, "live catalog returned no inspectable lossless files");
}

#[test]
fn archive_verifies_the_exact_torrent_and_keeps_popularity_separate() {
    let rows = json!({"response": {"docs": [
        {"identifier":"../bad", "creator":"X", "title":"Los Angeles"},
        {"identifier":"x-los-angeles", "creator":[null, "X"], "title":["Los Angeles"], "downloads":"99", "item_size":12345},
        {"identifier":"x-los-angeles-other", "creator":"X", "title":"Los Angeles"}
    ]}});
    let query = Query::new("X", "Los Angeles").unwrap();
    let hits = providers::archive::parse_search(&rows, &query).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "X – Los Angeles");
    assert_eq!(hits[0].seeders, None);
    assert_eq!(hits[0].popularity, Some(99));
    assert_eq!(hits[0].size_bytes, Some(12345));
    assert!(providers::archive::has_torrent(
        &json!({"files":[{"name":"x-los-angeles_archive.torrent"}]}),
        "x-los-angeles"
    )
    .unwrap());
    assert!(!providers::archive::has_torrent(
        &json!({"files":[{"name":"other_archive.torrent"}]}),
        "x-los-angeles"
    )
    .unwrap());
    assert!(!providers::archive::has_torrent(&json!([]), "x-los-angeles").unwrap());
    assert!(matches!(
        providers::archive::has_torrent(&json!({"error":"some upstream failure"}), "x-los-angeles"),
        Err(ProviderError::Unavailable)
    ));
}

#[tokio::test]
#[ignore = "live index and torrent metadata diagnostic"]
async fn live_release_label_and_tracklist_coverage() {
    use crate::torrent::matcher::{resolve_album_torrents, ExpectedTrackInput, TorrentResolveEvent};
    for (artist, title, tracks) in [
        ("Chris Christodoulou", "DEADBOLT", "Now I Am Become Death|Blood on the Dancefloor|This Parrot Is No More|Undead Man Walkin’|Heynong Man|The Great Beyond|The Proverbial Dust Biters|The Way of the Dodo|Reaper (and) Blues|Hemolysis|Werner und Klaus|The Banality of Eternity|Defunktorum™|Break of Dawn|Reaped by Death|Limbo|Dies Irae|Ashes to Ashes, to Ashes (to Ashes)|The Choir Invisible|Lux Perpetua"),
        ("Chris Christodoulou", "Risk of Rain", "Risk of Rain|Dew Point|Tropic of Capricorn|Monsoon|Cyclogenesis|25.3°N 91.7°E|Hailstorm|Moisture Deficit|Intermission|Tropic of Cancer|Aurora Borealis|Surface Tension|Arctic Oscillation|Precipitation|Double Fucking Rainbow|Coalescence|Chanson d'Automne.."),
        ("glass beach", "plastic death", "coelacanth|motions|slip under the door|guitar song|rare animal|cul-de-sac|whalefall|puppy|the killer|the CIA|200|commatose|abyss angel"),
        ("glass beach", "the first glass beach album", "classic j dies and goes to hell part 1|bedroom community|(forever---------)|bone skull|neon glow|cold weather|calico|glass beach|(blood rivers)|dallas|(rat castle)|planetarium|soft!!!!!!|yoshi's island|orchids"),
        ("Car Seat Headrest", "Living While Starving", "It's Only Sex|Reuse the Cels|I Hate Living|Devil Moon|Oh! Starving"),
        ("Car Seat Headrest", "Monomania", "Romantic Theory|Misheard Lyrics (Feat. Nora Knight)|Times To Die|Overexposed (Enjoy)|Los Borrachos (I Don’t Have Any Hope Left, But The Weather Is Nice)|Souls|Maud Gone|Sleeping With Strangers|Anchorite (Love You Very Much)"),
        ("Car Seat Headrest", "4", "a good bridge to never cross until there's no doubt that he's dead|who even knows|even the who knows|not “kidding” around|heartless dick|dickless heart|feel like daniel johnston|the ghost of bob saget|around"),
        ("Car Seat Headrest", "How To Leave Town", "The Ending of Dramamine|Beast Monster Thing (Love Isn't Love Enough)|Kimochi Warui (When? When? When? When? When? When? When?)|I-94 W (832 mi)|You're In Love With Me|America (Never Been)|I Want You To Know That I'm Awake/I Hope That You're Asleep|is this dust really from the Titanic?|Hey, Space Cadet (Beast Monster Thing In Space)"),
        ("Car Seat Headrest", "Making a Door Less Open", "Weightlifters|Can’t Cool Me Down|Deadlines (Hostile)|Hollywood|Hymn (remix)|Martin|Deadlines (Thoughtful)|What’s With You Lately|Life Worth Missing|There Must Be More Than Blood|Famous"),
        ("Car Seat Headrest", "Teens of Denial", "Fill in the Blank|Vincent|Destroyed by Hippie Powers|(Joe Gets Kicked Out of School for Using) Drugs With Friends (But Says This Isn't a Problem)|Not Just What I Needed|Drunk Drivers/Killer Whales|1937 State Park|Unforgiving Girl (She's Not An)|Cosmic Hero|The Ballad of the Costa Concordia|Connect the Dots (The Saga of Frank Sinatra)|Joe Goes to School"),
    ] {
        let expected = tracks.split('|').enumerate().map(|(index, track)| ExpectedTrackInput {
            index,
            title: track.into(),
            artist: Some(artist.into()),
            disc_number: Some(1),
            track_number: Some(index as u32 + 1),
            duration_ms: None,
        }).collect::<Vec<_>>();
        let events = std::sync::Mutex::new(Vec::new());
        let (resolved, unavailable) = resolve_album_torrents(artist, title, &expected, |event| {
            events.lock().unwrap().push(event);
        }).await.unwrap();
        println!("{artist} / {title}: unavailable={unavailable:?}");
        for release in &resolved {
            let web_seed = crate::torrent::web_seed::archive_item(&release.magnet_uri)
                .and_then(|item| {
                    let bytes = crate::torrent::inspect::cached_torrent_bytes(&release.magnet_uri)?;
                    crate::torrent::web_seed::ArchiveSeed::parse(&bytes, &item, &release.info_hash).ok()
                }).is_some();
            println!("  {}: {} {}/{} seeds={:?} web_seed={web_seed} missing={:?} ambiguous={:?}",
                release.provider, release.codec, release.matched_count, release.total_expected,
                release.seeders, release.missing_titles, release.ambiguous_titles);
        }
        let events = events.into_inner().unwrap();
        assert!(matches!(events.first(), Some(TorrentResolveEvent::Searching)));
        assert!(matches!(events.last(), Some(TorrentResolveEvent::Resolved { .. })));
        let checked: Vec<_> = events.iter().filter_map(|event| match event {
            TorrentResolveEvent::InspectionProgress { checked, total } => Some((*checked, *total)),
            _ => None,
        }).collect();
        if let Some(&(completed, total)) = checked.last() {
            assert_eq!(completed, total);
            assert!(checked.windows(2).all(|pair| pair[0].0 <= pair[1].0));
            assert!(checked.iter().all(|(checked, total)| checked <= total));
        }
    }
}

#[test]
fn knaben_accepts_magnet_only_and_hash_only_but_rejects_conflicting_identity() {
    let magnet = format!("magnet:?xt=urn:btih:{HASH}");
    let rows = json!({"hits": [
        {"title":"Artist - Song [FLAC]", "magnetUrl":magnet, "bytes":"152", "seeders":0},
        {"title":"Artist - Song [MP3]", "infoHash":OTHER_HASH, "seeders":"14"},
        {"title":"Artist - Song", "magnetUrl":magnet, "hash":OTHER_HASH},
        {"title":"Artist - Song", "hash":HASH, "infoHash":OTHER_HASH},
        {"title":"Artist - Song", "hash":"garbage"},
        {"name":"wrong property"}
    ]});
    let hits = providers::knaben::parse(&rows, &Query::new("Artist", "Song").unwrap()).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, format!("btih:{OTHER_HASH}"));
    assert_eq!(hits[0].seeders, Some(14));
    assert_eq!(hits[1].id, format!("btih:{HASH}"));
    assert_eq!(hits[1].seeders, Some(0));
    assert_eq!(hits[1].size_bytes, Some(152));
}

#[test]
fn knaben_keeps_a_document_source_without_passing_along_tracker_parameters() {
    let query = Query::new("Artist", "Song").unwrap();
    let magnet = format!("magnet:?xt=urn:btih:{HASH}&tr=https%3A%2F%2Fexample.org%2Fprivate&xs=https%3A%2F%2Fexample.org%2Fsong.torrent&xs=https%3A%2F%2Fexample.org%2Fsong.torrent%3Ftoken%3Dsecret");
    let hits = providers::knaben::parse(
        &json!({"hits":[
            {"title":"Artist - Song", "magnetUrl":magnet}
        ]}),
        &query,
    )
    .unwrap();
    let output = url::Url::parse(&hits[0].input).unwrap();
    assert_eq!(
        output.query_pairs().into_owned().collect::<Vec<_>>(),
        [
            ("xt".to_string(), format!("urn:btih:{HASH}")),
            ("xs".to_string(), "https://example.org/song.torrent".into()),
        ]
    );
}

#[test]
fn knaben_uses_a_validated_download_link_when_the_index_has_no_hash() {
    let query = Query::new("Artist", "Song").unwrap();
    let link = "https://knaben.eu/live/dl/indexer/?path=opaque-catalog-key&file=Song";
    let hits = providers::knaben::parse(&json!({"hits":[
        {"title":"Artist - Song", "hash":null, "magnetUrl":null, "link":link},
        {"title":"Artist - Song", "link":"https://other.example/live/dl/indexer/?path=unknown"},
        {"title":"Artist - Song", "link":"https://user@knaben.eu/live/dl/indexer/?path=unknown"},
        {"title":"Artist - Song", "link":"https://knaben.eu/live/dl/indexer/?path=opaque-catalog-key&file=Song"},
    ]}), &query).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].input, link);
    assert!(hits[0].id.starts_with("knaben:"));
    assert!(!hits[0].id.contains("opaque-catalog-key"));
    assert!(hits[0].info_hash.is_none());
}

#[test]
fn apibay_distinguishes_a_sentinel_from_an_invalid_response() {
    let zero_hash = "0".repeat(40);
    let query = Query::new("Artist", "Song").unwrap();
    let empty = json!([{"id":"0", "name":"No results returned", "info_hash": zero_hash}]);
    assert!(providers::apibay::parse(&empty, &query).unwrap().is_empty());
    let rows = json!([
        {"id":"123", "name":"Artist - Song", "info_hash":HASH, "size":42, "seeders":"0"},
        {"id":124, "name":"Artist - Song", "info_hash":OTHER_HASH, "size":"43"}
    ]);
    let hits = providers::apibay::parse(&rows, &query).unwrap();
    assert_eq!(hits[0].size_bytes, Some(42));
    assert_eq!(hits[0].seeders, Some(0));
    assert_eq!(hits[1].size_bytes, Some(43));
    assert_eq!(hits[1].seeders, None);
    let invalid = json!([{"id":"123", "info_hash":false}]);
    assert!(matches!(
        providers::apibay::parse(&invalid, &query),
        Err(ProviderError::Malformed)
    ));
}

#[tokio::test]
async fn progress_is_immediate_and_final_deduplication_ignores_completion_order() {
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    async fn collect_order(first: SearchHit, second: SearchHit) -> SearchHit {
        let provider = first.provider.clone();
        let (release_second, wait_for_first) = oneshot::channel();
        let release_second = Mutex::new(Some(release_second));
        let mut jobs = JoinSet::new();
        jobs.spawn(async move { ProviderReport::from_hits(first.provider.clone(), vec![first]) });
        jobs.spawn(async move {
            wait_for_first.await.unwrap();
            ProviderReport::from_hits(second.provider.clone(), vec![second])
        });
        let report = tokio::time::timeout(
            Duration::from_secs(2),
            collect(jobs, |event| {
                if let Some(send) = release_second.lock().unwrap().take() {
                    assert_eq!(event.provider, provider);
                    assert_eq!(event.hits[0].id, format!("btih:{HASH}"));
                    send.send(()).unwrap();
                }
            }),
        )
        .await
        .expect("the first result must arrive before the second job can finish");
        assert_eq!(report.providers.len(), 2);
        assert_eq!(report.hits.len(), 1);
        report.hits.into_iter().next().unwrap()
    }

    let query = Query::new("Artist", "Song").unwrap();
    let knaben = providers::knaben::parse(
        &json!({"hits":[
            {"title":"Artist - Song [FLAC]", "hash":HASH, "seeders":3}
        ]}),
        &query,
    )
    .unwrap()
    .remove(0);
    let apibay = providers::apibay::parse(
        &json!([
            {"id":"123", "name":"Artist - Song", "info_hash":HASH, "seeders":"8"}
        ]),
        &query,
    )
    .unwrap()
    .remove(0);

    let later_apibay = collect_order(knaben.clone(), apibay.clone()).await;
    let later_knaben = collect_order(apibay, knaben).await;
    for hit in [later_apibay, later_knaben] {
        assert_eq!(hit.provider, "ApiBay");
        assert_eq!(hit.providers, ["ApiBay", "Knaben"]);
        assert_eq!(hit.seeders, Some(8));
    }
}

fn serve_http(responses: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/search", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        for response in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "missing local HTTP request"
                        );
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("could not accept local HTTP request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = BufReader::new(&stream);
            loop {
                let mut line = String::new();
                assert_ne!(request.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (url, task)
}

fn wire_response(status: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{extra_headers}\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn transport_respects_retry_after_within_the_request_budget() {
    let client = Client::builder().no_proxy().build().unwrap();
    let gate = providers::Gate::new();
    let date = httpdate::fmt_http_date(std::time::SystemTime::now() - Duration::from_secs(1));
    let (url, server) = serve_http(vec![
        wire_response(
            "429 Too Many Requests",
            &format!("Retry-After: {date}\r\n"),
            "",
        ),
        wire_response("200 OK", "", r#"{"hits":[]}"#),
    ]);
    let response = gate
        .json(
            client.get(&url),
            tokio::time::Instant::now() + Duration::from_secs(3),
        )
        .await
        .unwrap();
    assert_eq!(response, json!({"hits":[]}));
    server.join().unwrap();

    let gate = providers::Gate::new();
    let (url, server) = serve_http(vec![wire_response(
        "429 Too Many Requests",
        "Retry-After: 60\r\n",
        "",
    )]);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    assert_eq!(
        gate.json(client.get(&url), deadline).await,
        Err(ProviderError::RateLimited)
    );
    server.join().unwrap();
    assert_eq!(
        gate.json(client.get(&url), deadline).await,
        Err(ProviderError::RateLimited)
    );
}

#[tokio::test]
async fn transport_reports_http_failures_and_rejects_malformed_or_oversized_bodies() {
    let client = Client::builder().no_proxy().build().unwrap();
    for (response, expected) in [
        (
            wire_response("503 Service Unavailable", "", ""),
            ProviderError::Unavailable,
        ),
        (
            wire_response("200 OK", "", "<!doctype html>"),
            ProviderError::Malformed,
        ),
        (
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 40\r\n\r\n{}".to_string(),
            ProviderError::Malformed,
        ),
        (
            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5000000\r\n\r\n".to_string(),
            ProviderError::ResponseTooLarge,
        ),
    ] {
        let (url, server) = serve_http(vec![response]);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        assert_eq!(
            providers::Gate::new()
                .json(client.get(&url), deadline)
                .await,
            Err(expected)
        );
        server.join().unwrap();
    }
}
