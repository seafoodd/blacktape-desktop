//! Comprehensive search relevance simulation suite (75+ real-world search scenarios across
//! exact, album-intent, split, reversed, partial, stemmed/alias, niche, and hostile bootleg/remix inputs).
//! Exercises both `rank_with_options` (search bar dropdown) and `rank_flat_with_options` (full search page).

use super::ItemType;
use super::full::{assemble_with_canonical, Fetched};
use super::ranking::{normalize, rank_flat_with_options, rank_with_options, RankOptions};
use super::youtube_fast::merge_album_results;
use super::SearchSuggestion;
use crate::types::Platform;

fn sug(
    item_type: ItemType,
    name: &str,
    band: &str,
    platform: Platform,
    url: &str,
    views: Option<u64>,
    subs: Option<u64>,
) -> SearchSuggestion {
    SearchSuggestion {
        item_type,
        name: name.to_string(),
        band_name: band.to_string(),
        album_name: None,
        item_url_path: url.to_string(),
        img: "https://img.test/cover.jpg".to_string(),
        subscriber_count: subs,
        audience_kind: subs.map(|_| "monthly_audience"),
        album_url: None,
        view_count: views,
        year: Some(2015),
        duration: Some(210),
        platform,
    }
}

fn mb_album(title: &str, artist: &str, mbid: &str) -> SearchSuggestion {
    sug(
        ItemType::Album,
        title,
        artist,
        Platform::Youtube,
        &format!("canonical:{mbid}"),
        None,
        None,
    )
}

fn ytm_album(title: &str, artist: &str, id: &str) -> SearchSuggestion {
    sug(
        ItemType::Album,
        title,
        artist,
        Platform::Youtube,
        &format!("https://music.youtube.com/browse/{id}"),
        Some(5_000_000),
        None,
    )
}

fn bc_album(title: &str, artist: &str, slug: &str) -> SearchSuggestion {
    sug(
        ItemType::Album,
        title,
        artist,
        Platform::Bandcamp,
        &format!("https://{slug}.bandcamp.com/album/a"),
        None,
        None,
    )
}

fn sc_album(title: &str, artist: &str, slug: &str) -> SearchSuggestion {
    sug(
        ItemType::Album,
        title,
        artist,
        Platform::Soundcloud,
        &format!("https://soundcloud.com/{slug}/sets/s"),
        Some(1_200),
        None,
    )
}

fn ytm_track(title: &str, artist: &str, id: &str, views: u64) -> SearchSuggestion {
    sug(
        ItemType::Track,
        title,
        artist,
        Platform::Youtube,
        &format!("https://youtube.com/watch?v={id}"),
        Some(views),
        None,
    )
}

fn sc_track(title: &str, artist: &str, slug: &str, views: u64) -> SearchSuggestion {
    sug(
        ItemType::Track,
        title,
        artist,
        Platform::Soundcloud,
        &format!("https://soundcloud.com/{slug}"),
        Some(views),
        None,
    )
}

fn bc_track(title: &str, artist: &str, slug: &str) -> SearchSuggestion {
    sug(
        ItemType::Track,
        title,
        artist,
        Platform::Bandcamp,
        &format!("https://{slug}.bandcamp.com/track/t"),
        None,
        None,
    )
}

fn ytm_artist(name: &str, id: &str, subs: u64) -> SearchSuggestion {
    sug(
        ItemType::Artist,
        name,
        "Artist",
        Platform::Youtube,
        &format!("https://youtube.com/browse/{id}"),
        None,
        Some(subs),
    )
}

fn run_dropdown(query: &str, raw: Vec<SearchSuggestion>) -> Vec<SearchSuggestion> {
    rank_with_options(
        raw,
        query,
        RankOptions {
            promote_canonical_albums: true,
        },
    )
}

fn run_full(query: &str, raw: Vec<SearchSuggestion>) -> Vec<SearchSuggestion> {
    rank_flat_with_options(
        raw,
        query,
        RankOptions {
            promote_canonical_albums: true,
        },
    )
    .0
}

fn assert_top_match(
    scenario: &str,
    query: &str,
    candidates: Vec<SearchSuggestion>,
    expected_type: ItemType,
    expected_name_norm: &str,
    expected_band_norm: &str,
) {
    let dropdown = run_dropdown(query, candidates.clone());
    assert!(
        !dropdown.is_empty(),
        "[{scenario}] dropdown empty for query '{query}'"
    );
    let top = &dropdown[0];
    assert_eq!(
        top.item_type, expected_type,
        "[{scenario}] dropdown top wrong type for '{query}': got {:?} '{}' by '{}'",
        top.item_type, top.name, top.band_name
    );
    assert!(
        normalize(&top.name).contains(expected_name_norm),
        "[{scenario}] dropdown top title mismatch for '{query}': got '{}' (norm '{}'), expected contains '{}'",
        top.name,
        normalize(&top.name),
        expected_name_norm
    );
    if !expected_band_norm.is_empty() {
        assert!(
            normalize(&top.band_name).contains(expected_band_norm),
            "[{scenario}] dropdown top artist mismatch for '{query}': got '{}' (norm '{}'), expected contains '{}'",
            top.band_name,
            normalize(&top.band_name),
            expected_band_norm
        );
    }

    // Also verify the Full Search Page (`rank_flat_with_options`) for the same kind!
    let same_kind: Vec<SearchSuggestion> = candidates
        .into_iter()
        .filter(|c| c.item_type == expected_type)
        .collect();
    let full = run_full(query, same_kind);
    assert!(
        !full.is_empty(),
        "[{scenario}] full search empty for query '{query}'"
    );
    let full_top = &full[0];
    assert!(
        normalize(&full_top.name).contains(expected_name_norm)
            && (expected_band_norm.is_empty()
                || normalize(&full_top.band_name).contains(expected_band_norm)),
        "[{scenario}] full search top mismatch for '{query}': got '{}' by '{}', expected '{}' by '{}'",
        full_top.name,
        full_top.band_name,
        expected_name_norm,
        expected_band_norm
    );
}

#[test]
fn simulation_screenshot_regressions_must_all_pass() {
    // 1. User Screenshot 1: "death grips no love deep web"
    assert_top_match(
        "screenshot-1 no love deep web",
        "death grips no love deep web",
        vec![
            bc_album(
                "Death Grips | No Love Deep Web (Chopped)",
                "The Mech Cannibal",
                "mech",
            ),
            ytm_track("No Love", "Death Grips", "nl1", 16_000_000),
            ytm_track("Deep Web", "Death Grips", "dw1", 4_000_000),
            sc_track("Vocals from Death Grips- 'No Love Deep Web'", "Hashman", "hash", 388),
            bc_album("NO LOVE DEEP WEB", "Death Grips", "deathgrips"),
            ytm_album("No Love Deep Web", "Death Grips", "MPRE_nldw"),
            mb_album("NO LOVE DEEP WEB", "Death Grips", "mbid-nldw"),
            sc_track(
                "How Deep Web Is Your Love - Death Grips v Bee Gees",
                "No Love Bee Gees",
                "mash",
                900,
            ),
        ],
        ItemType::Album,
        "no love deep web",
        "death grips",
    );

    // 2. User Screenshot 2: "the doors albums"
    assert_top_match(
        "screenshot-2 the doors albums",
        "the doors albums",
        vec![
            bc_album("The Doors", "Albums But Two Minutes", "abtm"),
            bc_track("The Doors", "Albums But Two Minutes", "abtm-t"),
            ytm_track("Hello, I Love You (New Stereo Mix)", "The Doors", "h1", 25_000_000),
            ytm_track("Break on Through (To the Other Side)", "The Doors", "b1", 90_000_000),
            ytm_album("The Doors", "The Doors", "MPRE_doors"),
            mb_album("The Doors", "The Doors", "mbid-doors"),
        ],
        ItemType::Album,
        "the doors",
        "the doors",
    );

    // 3. User Screenshot 3: "the doors strange"
    assert_top_match(
        "screenshot-3 the doors strange",
        "the doors strange",
        vec![
            bc_album(
                "THE DOORS STRANGE DAYS FULL ALBUM",
                "Jason Voorhees",
                "jason",
            ),
            ytm_track("People Are Strange (Live at The Matrix)", "The Doors", "pas-live", 1_000_000),
            ytm_track("Strange (Mortisville Remix)", "The Doors", "s-remix", 50_000),
            ytm_album("Strange Days", "The Doors", "MPRE_sd"),
            mb_album("Strange Days", "The Doors", "mbid-sd"),
        ],
        ItemType::Album,
        "strange days",
        "the doors",
    );

    // 4. User Screenshot 4: "the beatle white album"
    assert_top_match(
        "screenshot-4 the beatle white album",
        "the beatle white album",
        vec![
            bc_album("The Beatles White Album", "Rodrigo Constanzo", "rodrigo"),
            sc_track("Bad white beatle", "The Cold Dicks", "colddicks", 120),
            sc_track("White Album", "xienli", "xienli", 450),
            sc_track("WHITE ALBUM", "Ohma Shu", "ohmashu", 800),
            bc_album("WHITE ALBUM", "VELTPUNCH", "veltpunch"),
            ytm_album("The Beatles (Remastered 2009)", "The Beatles", "MPRE_white"),
            mb_album("The Beatles", "The Beatles", "mbid-white"),
        ],
        ItemType::Album,
        "the beatles",
        "the beatles",
    );
}

#[test]
fn simulation_80_plus_real_world_searches() {
    // Format: (scenario, query, expected_type, expected_title_substr, expected_artist_substr, official_item)
    // Each scenario automatically pits the official release against realistic SoundCloud/Bandcamp junk,
    // bootlegs, remixes, covers, and unrelated hits!
    let album_scenarios: Vec<(&str, &str, &str, &str, SearchSuggestion)> = vec![
        ("daft-punk-discovery", "daft punk discovery", "discovery", "daft punk", mb_album("Discovery", "Daft Punk", "mb1")),
        ("daft-punk-ram", "daft punk random access memories", "random access memories", "daft punk", ytm_album("Random Access Memories", "Daft Punk", "yt2")),
        ("daft-punk-homework", "daft punk homework", "homework", "daft punk", mb_album("Homework", "Daft Punk", "mb3")),
        ("radiohead-in-rainbows", "radiohead in rainbows", "in rainbows", "radiohead", mb_album("In Rainbows", "Radiohead", "mb4")),
        ("radiohead-ok-computer", "radiohead ok computer", "ok computer", "radiohead", ytm_album("OK Computer", "Radiohead", "yt5")),
        ("radiohead-kid-a", "radiohead kid a", "kid a", "radiohead", mb_album("Kid A", "Radiohead", "mb6")),
        ("aphex-drukqs", "aphex twin drukqs", "drukqs", "aphex twin", mb_album("Drukqs", "Aphex Twin", "mb7")),
        ("aphex-saw-85", "aphex twin selected ambient works", "selected ambient works", "aphex twin", ytm_album("Selected Ambient Works 85-92", "Aphex Twin", "yt8")),
        ("aphex-rdj", "aphex twin richard d james album", "richard d james album", "aphex twin", mb_album("Richard D. James Album", "Aphex Twin", "mb9")),
        ("pink-floyd-dsotm", "pink floyd dark side of the moon", "dark side of the moon", "pink floyd", ytm_album("The Dark Side of the Moon", "Pink Floyd", "yt10")),
        ("pink-floyd-wywh", "pink floyd wish you were here", "wish you were here", "pink floyd", mb_album("Wish You Were Here", "Pink Floyd", "mb11")),
        ("pink-floyd-animals", "pink floyd animals", "animals", "pink floyd", mb_album("Animals", "Pink Floyd", "mb12")),
        ("nirvana-nevermind", "nirvana nevermind", "nevermind", "nirvana", ytm_album("Nevermind", "Nirvana", "yt13")),
        ("nirvana-in-utero", "nirvana in utero", "in utero", "nirvana", mb_album("In Utero", "Nirvana", "mb14")),
        ("kendrick-gkmc", "kendrick lamar good kid maad city", "good kid", "kendrick lamar", ytm_album("good kid, m.A.A.d city", "Kendrick Lamar", "yt15")),
        ("kendrick-tpab", "kendrick lamar to pimp a butterfly", "to pimp a butterfly", "kendrick lamar", mb_album("To Pimp a Butterfly", "Kendrick Lamar", "mb16")),
        ("kanye-mbdtf", "kanye west my beautiful dark twisted fantasy", "my beautiful dark twisted fantasy", "kanye west", ytm_album("My Beautiful Dark Twisted Fantasy", "Kanye West", "yt17")),
        ("kanye-yeezus", "kanye west yeezus", "yeezus", "kanye west", mb_album("Yeezus", "Kanye West", "mb18")),
        ("tyler-igor", "tyler the creator igor", "igor", "tyler the creator", mb_album("IGOR", "Tyler, The Creator", "mb19")),
        ("tyler-flower-boy", "tyler the creator flower boy", "flower boy", "tyler the creator", ytm_album("Flower Boy", "Tyler, The Creator", "yt20")),
        ("frank-ocean-blonde", "frank ocean blonde", "blonde", "frank ocean", mb_album("Blonde", "Frank Ocean", "mb21")),
        ("frank-channel-orange", "frank ocean channel orange", "channel orange", "frank ocean", ytm_album("channel ORANGE", "Frank Ocean", "yt22")),
        ("mf-doom-madvillainy", "madvillain madvillainy", "madvillainy", "madvillain", mb_album("Madvillainy", "Madvillain", "mb23")),
        ("mf-doom-mm-food", "mf doom mm food", "mm food", "mf doom", ytm_album("MM..FOOD", "MF DOOM", "yt24")),
        ("tame-impala-currents", "tame impala currents", "currents", "tame impala", mb_album("Currents", "Tame Impala", "mb25")),
        ("tame-impala-lonerism", "tame impala lonerism", "lonerism", "tame impala", ytm_album("Lonerism", "Tame Impala", "yt26")),
        ("bjork-post", "bjork post", "post", "bjork", mb_album("Post", "Björk", "mb27")),
        ("bjork-homogenic", "bjork homogenic", "homogenic", "bjork", ytm_album("Homogenic", "Björk", "yt28")),
        ("gorillaz-demon-days", "gorillaz demon days", "demon days", "gorillaz", mb_album("Demon Days", "Gorillaz", "mb29")),
        ("gorillaz-plastic-beach", "gorillaz plastic beach", "plastic beach", "gorillaz", ytm_album("Plastic Beach", "Gorillaz", "yt30")),
        ("sigur-ros-agaetis", "sigur ros agaetis byrjun", "agaetis byrjun", "sigur ros", mb_album("Ágætis byrjun", "Sigur Rós", "mb31")),
        ("boc-geogaddi", "boards of canada geogaddi", "geogaddi", "boards of canada", mb_album("Geogaddi", "Boards of Canada", "mb32")),
        ("boc-music-has-the-right", "boards of canada music has the right to children", "music has the right to children", "boards of canada", ytm_album("Music Has the Right to Children", "Boards of Canada", "yt33")),
        ("mbv-loveless", "my bloody valentine loveless", "loveless", "my bloody valentine", mb_album("loveless", "my bloody valentine", "mb34")),
        ("nmh-aeroplane", "neutral milk hotel in the aeroplane over the sea", "in the aeroplane over the sea", "neutral milk hotel", mb_album("In the Aeroplane Over the Sea", "Neutral Milk Hotel", "mb35")),
        ("miles-kind-of-blue", "miles davis kind of blue", "kind of blue", "miles davis", ytm_album("Kind of Blue", "Miles Davis", "yt36")),
        ("coltrane-love-supreme", "john coltrane a love supreme", "a love supreme", "john coltrane", mb_album("A Love Supreme", "John Coltrane", "mb37")),
        ("bowie-ziggy", "david bowie ziggy stardust", "ziggy stardust", "david bowie", mb_album("The Rise and Fall of Ziggy Stardust and the Spiders from Mars", "David Bowie", "mb38")),
        ("acdc-back-in-black", "ac dc back in black", "back in black", "ac dc", ytm_album("Back in Black", "AC/DC", "yt39")),
        ("gnr-appetite", "guns n roses appetite for destruction", "appetite for destruction", "guns n roses", mb_album("Appetite for Destruction", "Guns N' Roses", "mb40")),
        ("metallica-puppets", "metallica master of puppets", "master of puppets", "metallica", mb_album("Master of Puppets", "Metallica", "mb41")),
        ("metallica-black-alias", "metallica black album", "metallica", "metallica", mb_album("Metallica", "Metallica", "mb42")),
        ("weezer-blue-alias", "weezer blue album", "weezer", "weezer", ytm_album("Weezer", "Weezer", "yt43")),
        // Intent-modified album queries
        ("intent-dg-tms", "death grips the money store album", "the money store", "death grips", mb_album("The Money Store", "Death Grips", "mb44")),
        ("intent-dg-bp", "death grips bottomless pit albums", "bottomless pit", "death grips", ytm_album("Bottomless Pit", "Death Grips", "yt45")),
        ("intent-radiohead-ep", "radiohead in rainbows discography", "in rainbows", "radiohead", mb_album("In Rainbows", "Radiohead", "mb46")),
        // Reversed order (<album> <artist>)
        ("reversed-in-rainbows", "in rainbows radiohead", "in rainbows", "radiohead", mb_album("In Rainbows", "Radiohead", "mb47")),
        ("reversed-discovery", "discovery daft punk", "discovery", "daft punk", ytm_album("Discovery", "Daft Punk", "yt48")),
        ("reversed-madvillainy", "madvillainy madvillain", "madvillainy", "madvillain", mb_album("Madvillainy", "Madvillain", "mb49")),
        // Prefix / partial album titles
        ("partial-dg-money", "death grips money", "the money store", "death grips", mb_album("The Money Store", "Death Grips", "mb50")),
        ("partial-pink-floyd-dark", "pink floyd dark side", "dark side of the moon", "pink floyd", ytm_album("The Dark Side of the Moon", "Pink Floyd", "yt51")),
        // Niche / underground Bandcamp & experimental artists
        ("niche-glass-beach", "glass beach plastic death", "plastic death", "glass beach", bc_album("plastic death", "glass beach", "glassbeach")),
        ("niche-panchiko", "panchiko deathmetal", "deathmetal", "panchiko", bc_album("D>E>A>T>H>M>E>T>A>L", "Panchiko", "panchiko")),
        ("niche-sweet-trip", "sweet trip velocity design comfort", "velocity design comfort", "sweet trip", mb_album("velocity : design : comfort", "Sweet Trip", "mb54")),
        ("niche-fishmans", "fishmans long season", "long season", "fishmans", mb_album("LONG SEASON", "Fishmans", "mb55")),
        ("niche-mid-air-thief", "mid air thief crumbling", "crumbling", "mid air thief", bc_album("Crumbling", "Mid-Air Thief", "midairthief")),
        ("niche-weatherday", "weatherday come in", "come in", "weatherday", bc_album("Come In", "Weatherday", "weatherday")),
        ("niche-machine-girl", "machine girl wlfgrl", "wlfgrl", "machine girl", bc_album("WLFGRL", "Machine Girl", "machinegirl")),
        ("niche-sewerslvt", "sewerslvt draining love story", "draining love story", "sewerslvt", bc_album("Draining Love Story", "Sewerslvt", "sewerslvt")),
        ("niche-boris", "boris feedbacker", "feedbacker", "boris", mb_album("Boris at Last -Feedbacker-", "Boris", "mb60")),
    ];

    for (idx, (scenario, query, exp_title, exp_artist, official)) in album_scenarios.into_iter().enumerate() {
        let mut pool = vec![
            // Bootleg uploader cramming artist + album in title with separator
            bc_album(
                &format!("{exp_artist} | {exp_title} (Chopped & Screwed)"),
                "Bootleg Kingz",
                &format!("boot{idx}"),
            ),
            // Full album spammer on YouTube / Bandcamp
            bc_album(
                &format!("{exp_artist} {exp_title} FULL ALBUM"),
                "VinylRipper99",
                &format!("rip{idx}"),
            ),
            // Derivative remix album
            sc_album(
                &format!("{exp_title} (8D Slowed Remix)"),
                exp_artist,
                &format!("rem{idx}"),
            ),
            // Unrelated SoundCloud track sharing query keywords
            sc_track(
                &format!("vocals from {exp_artist} {exp_title} cover"),
                "BedroomProducer",
                &format!("sc{idx}"),
                240,
            ),
            // Joke artist matching album keyword
            bc_album(exp_artist, "Albums But Faster", &format!("joke{idx}")),
            // Official single track from the album (should sit below album for exact album query)
            ytm_track("Track 1", exp_artist, &format!("tr{idx}"), 500_000),
            official,
        ];
        // Shuffle worst-first order to prove ranking never depends on arrival order
        pool.reverse();
        assert_top_match(scenario, query, pool, ItemType::Album, exp_title, exp_artist);
    }

    // Track-focused queries (12 scenarios)
    let track_scenarios = vec![
        ("track-one-more-time", "daft punk one more time", "one more time", "daft punk"),
        ("track-around-the-world", "daft punk around the world", "around the world", "daft punk"),
        ("track-roygbiv", "boards of canada roygbiv", "roygbiv", "boards of canada"),
        ("track-avril-14", "aphex twin avril 14th", "avril 14th", "aphex twin"),
        ("track-weird-fishes", "radiohead weird fishes", "weird fishes", "radiohead"),
        ("track-no-love", "death grips no love", "no love", "death grips"),
        ("track-hacker", "death grips hacker", "hacker", "death grips"),
        ("track-people-strange", "the doors people are strange", "people are strange", "the doors"),
        ("track-riders-storm", "the doors riders on the storm", "riders on the storm", "the doors"),
        ("track-time", "pink floyd time", "time", "pink floyd"),
        ("track-let-it-happen", "tame impala let it happen", "let it happen", "tame impala"),
        ("track-niggas-paris", "kendrick lamar swimming pools", "swimming pools", "kendrick lamar"),
    ];
    for (idx, (scenario, query, exp_title, exp_artist)) in track_scenarios.into_iter().enumerate() {
        let pool = vec![
            sc_track(&format!("{exp_artist} - {exp_title} (Slowed + Reverb)"), "RandomUploader", &format!("su{idx}"), 999),
            bc_track(&format!("{exp_title} Piano Cover"), "CoverArtist", &format!("bc{idx}")),
            ytm_track(&format!("{exp_title} (Mortis Remix)"), exp_artist, &format!("rmx{idx}"), 5_000),
            ytm_track(&format!("{exp_title} (Live)"), exp_artist, &format!("liv{idx}"), 200_000),
            ytm_artist(exp_artist, &format!("art{idx}"), 1_000_000),
            ytm_track(exp_title, exp_artist, &format!("off{idx}"), 25_000_000),
        ];
        assert_top_match(scenario, query, pool, ItemType::Track, exp_title, exp_artist);
    }

    // Artist-only queries (8 scenarios)
    let artist_scenarios = vec![
        ("artist-daft-punk", "daft punk", "daft punk"),
        ("artist-death-grips", "death grips", "death grips"),
        ("artist-the-doors", "the doors", "the doors"),
        ("artist-the-beatles", "the beatles", "the beatles"),
        ("artist-radiohead", "radiohead", "radiohead"),
        ("artist-aphex-twin", "aphex twin", "aphex twin"),
        ("artist-pink-floyd", "pink floyd", "pink floyd"),
        ("artist-tame-impala", "tame impala", "tame impala"),
    ];
    for (idx, (scenario, query, exp_artist)) in artist_scenarios.into_iter().enumerate() {
        let pool = vec![
            ytm_track(&format!("{exp_artist} Type Beat"), "Producer99", &format!("tb{idx}"), 10_000_000),
            bc_album(&format!("{exp_artist} Tribute"), "Tribute Band", &format!("trb{idx}")),
            ytm_track("Biggest Hit", exp_artist, &format!("bh{idx}"), 50_000_000),
            ytm_artist(exp_artist, &format!("ar{idx}"), 10_000_000),
        ];
        assert_top_match(scenario, query, pool, ItemType::Artist, exp_artist, "");
    }

    // Explicit remix request when user DOES ask for a remix (must allow remix to rank #1!)
    assert_top_match(
        "explicit-remix-requested",
        "daft punk one more time remix",
        vec![
            ytm_track("One More Time", "Daft Punk", "orig1", 100_000_000),
            ytm_track("One More Time (Club Remix)", "Daft Punk", "rmx1", 500_000),
        ],
        ItemType::Track,
        "remix",
        "daft punk",
    );
}

#[test]
fn simulation_full_search_keeps_canonical_copies_synced_and_merges_yt_albums() {
    let yt = Ok(Fetched {
        items: vec![ytm_album("No Love Deep Web", "Death Grips", "MPRE_nldw")],
        ..Fetched::default()
    });
    let bc = Ok(Fetched {
        items: vec![bc_album("NO LOVE DEEP WEB", "Death Grips", "dg")],
        ..Fetched::default()
    });
    let mb = vec![mb_album("NO LOVE DEEP WEB", "Death Grips", "mbid-nldw")];

    let page = assemble_with_canonical("death grips no love deep web", Some(yt), Some(bc), None, mb)
        .expect("page should assemble");

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].item_url_path, "canonical:mbid-nldw");
    assert_eq!(page.copies.len(), 1, "Bandcamp copy must be preserved");
    assert_eq!(
        page.copies[0].of, page.items[0].item_url_path,
        "copy.of MUST stay synchronized with kept canonical URL for platform filtering"
    );
    assert!(
        page.copies[0].item.item_url_path.starts_with("canonical:"),
        "copy album must also open in canonical preview"
    );

    // Also test merge_album_results preserves existing items and merges unique albums only
    let base = vec![
        ytm_track("No Love", "Death Grips", "t1", 1000),
        ytm_album("The Money Store", "Death Grips", "a1"),
    ];
    let albums = vec![
        ytm_album("The Money Store", "Death Grips", "a1"), // dup
        ytm_album("No Love Deep Web", "Death Grips", "a2"), // new album
        ytm_track("Ignored Track", "X", "t2", 1),          // non-album ignored
    ];
    let merged = merge_album_results(base, albums);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[2].name, "No Love Deep Web");
    // Also verify dedup borrows reliable CDN covers over coverartarchive.org and increases album slots on Album intent
    let mut ranked_with_cover = run_dropdown(
        "death grips no love deep web",
        vec![
            SearchSuggestion {
                img: "https://coverartarchive.org/release/mbid-1/front-250".to_string(),
                year: None,
                ..mb_album("NO LOVE DEEP WEB", "Death Grips", "mbid-1")
            },
            SearchSuggestion {
                img: "https://lh3.googleusercontent.com/yt-cover=w544-h544".to_string(),
                year: Some(2012),
                ..ytm_album("No Love Deep Web", "Death Grips", "MPRE_nldw")
            },
        ],
    );
    assert_eq!(ranked_with_cover.len(), 1);
    assert_eq!(
        ranked_with_cover[0].img,
        "https://lh3.googleusercontent.com/yt-cover=w544-h544"
    );
    assert_eq!(ranked_with_cover[0].year, Some(2012));

    // Also test Album intent promotes up to 4 albums ahead of tracks
    let mut doors_raw = vec![
        ytm_track("Break On Through", "The Doors", "b1", 10_000_000),
        ytm_track("Light My Fire", "The Doors", "l1", 10_000_000),
        ytm_track("Riders on the Storm", "The Doors", "r1", 10_000_000),
        ytm_album("The Doors", "The Doors", "a1"),
        ytm_album("Strange Days", "The Doors", "a2"),
        ytm_album("L.A. Woman", "The Doors", "a3"),
        ytm_album("Morrison Hotel", "The Doors", "a4"),
    ];
    doors_raw.reverse();
    ranked_with_cover = run_dropdown("the doors albums", doors_raw);
    assert!(ranked_with_cover[..4]
        .iter()
        .all(|item| item.item_type == ItemType::Album));
}

#[test]
fn regression_morrison_hotel_beats_bandcamp_homonym_band() {
    let raw = vec![
        bc_album("Artie's waltz", "Morrison Hotel", "mh"),
        bc_album("1st.", "Morrison Hotel", "mh"),
        bc_track("Oasis", "Morrison Hotel", "mh"),
        bc_track("Sailing", "Morrison Hotel", "mh"),
        bc_track("Lullaby", "Morrison Hotel", "mh"),
        SearchSuggestion {
            item_type: ItemType::Artist,
            name: "Morrison Hotel".to_string(),
            band_name: "Morrison Hotel".to_string(),
            album_name: None,
            item_url_path: "https://morrisonhotel.bandcamp.com".to_string(),
            img: String::new(),
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            duration: None,
            platform: Platform::Bandcamp,
        },
        SearchSuggestion {
            view_count: Some(1_750_000),
            ..mb_album("Morrison Hotel", "The Doors", "rg:deee6f92")
        },
        ytm_album("Morrison Hotel", "The Doors", "MPRE_mh"),
    ];

    let dropdown = run_dropdown("morrison hotel", raw.clone());
    assert_eq!(dropdown[0].item_type, ItemType::Album);
    assert_eq!(dropdown[0].name, "Morrison Hotel");
    assert_eq!(dropdown[0].band_name, "The Doors");
    assert!(dropdown[0].item_url_path.starts_with("canonical:"));

    // Lone 1-platform Bandcamp albums ("Artie's waltz", "1st.") must NOT claim MULTI-SOURCE.
    for item in &dropdown {
        if item.name == "Artie's waltz" || item.name == "1st." {
            assert!(
                !item.item_url_path.starts_with("canonical:"),
                "single-platform Bandcamp album '{}' must not be marked canonical/MULTI-SOURCE",
                item.name
            );
        }
    }

    let album_page_items = run_full(
        "morrison hotel",
        raw.into_iter()
            .filter(|i| i.item_type == ItemType::Album)
            .collect(),
    );
    assert_eq!(album_page_items[0].name, "Morrison Hotel");
    assert_eq!(album_page_items[0].band_name, "The Doors");
}

#[test]
fn regression_aphex_twin_and_doors_albums_crush_parody_uploads() {
    let raw = vec![
        bc_track("aphex twin does dubstep", "aphex twin", "troll1"),
        bc_track("Aphex Twin - Alberto Balsalm piano cover", "Aphex Twin", "cover1"),
        bc_album("aphex twin is back baybe", "aphex twin", "troll2"),
        ytm_track("Alberto Balsalm", "Aphex Twin", "yt_alb", 5_000_000),
        SearchSuggestion {
            view_count: Some(1_200_000),
            ..mb_album("Drukqs", "Aphex Twin", "rg:drukqs")
        },
        ytm_artist("Aphex Twin", "UC_aphex", 543_000),
    ];

    let dropdown = run_dropdown("aphex twin", raw.clone());
    assert_eq!(
        dropdown[0].item_type,
        ItemType::Artist,
        "famous Artist should hold champion #1"
    );
    assert_eq!(dropdown[0].name, "Aphex Twin");

    let track_positions: Vec<&str> = dropdown
        .iter()
        .filter(|i| i.item_type == ItemType::Track)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(track_positions[0], "Alberto Balsalm");

    let drukqs = dropdown
        .iter()
        .find(|i| i.name == "Drukqs")
        .expect("Drukqs present");
    let troll_album = dropdown
        .iter()
        .find(|i| i.name == "aphex twin is back baybe")
        .expect("troll album present");
    let drukqs_idx = dropdown.iter().position(|i| i.name == "Drukqs").unwrap();
    let troll_idx = dropdown
        .iter()
        .position(|i| i.name == "aphex twin is back baybe")
        .unwrap();
    assert!(drukqs_idx < troll_idx, "Drukqs must rank above parody album");
    assert!(drukqs.item_url_path.starts_with("canonical:"));
    assert!(
        !troll_album.item_url_path.starts_with("canonical:"),
        "single-platform Bandcamp upload must keep BANDCAMP badge, not MULTI-SOURCE"
    );

    // Multi-platform merge (Bandcamp + YouTube without MB) promotes honestly to MULTI-SOURCE.
    let multi = vec![
        bc_album("Selected Ambient Works 85-92", "Aphex Twin", "saw"),
        ytm_album("Selected Ambient Works 85-92", "Aphex Twin", "MPRE_saw"),
    ];
    let (page_items, page_copies) = rank_flat_with_options(
        multi,
        "selected ambient works",
        RankOptions {
            promote_canonical_albums: true,
        },
    );
    assert_eq!(page_items.len(), 1);
    assert!(page_items[0].item_url_path.starts_with("canonical:"));
    assert_eq!(page_copies.len(), 1);
    assert_eq!(page_copies[0].0, page_items[0].item_url_path);

    // "the doors albums" must rank The Doors (The Doors) ahead of parody "Albums But Two Minutes".
    let doors = vec![
        bc_album("The Doors", "Albums But Two Minutes", "ab2m"),
        SearchSuggestion {
            view_count: Some(2_500_000),
            ..mb_album("The Doors", "The Doors", "rg:doors_st")
        },
        ytm_album("The Doors", "The Doors", "MPRE_doors"),
    ];
    let doors_dd = run_dropdown("the doors albums", doors);
    assert_eq!(doors_dd[0].band_name, "The Doors");
    assert_eq!(doors_dd[0].name, "The Doors");

    // "plastic death": official multi-source studio album by glass beach must rank #1 ahead of
    // single-platform tracks and unrelated EPs/singles (both with MusicBrainz AND when MB fails).
    let plastic_death_raw = vec![
        bc_track("Plastic Death", "Strange Arrival", "sa"),
        bc_album("Plastic Death", "Strange Arrival", "sa_alb"),
        bc_track("Plastic Death", "Rory Sweeney", "rs"),
        sc_track("Plastic Death", "Rory Sweeney", "rs_sc", 5_000),
        ytm_track("Plastic", "deadforest", "yt_pd", 120_000),
        bc_album("plastic death", "glass beach", "gb_pd"),
        ytm_album("plastic death", "glass beach", "MPRE_gb_pd"),
        mb_album("plastic death", "glass beach", "rg:gb_pd"),
        sc_track("Plastic Lovin", "Death the Kid", "dtk", 50_000),
        bc_track("Death to the Slumlords", "Plastic Handgun", "ph"),
        ytm_album("Plastic Cigarette", "Death By Neglect", "MPRE_dbn"),
    ];
    let pd_with_mb = run_dropdown("plastic death", plastic_death_raw.clone());
    assert_eq!(pd_with_mb[0].item_type, ItemType::Album);
    assert_eq!(pd_with_mb[0].band_name, "glass beach");
    assert!(pd_with_mb[0].item_url_path.starts_with("canonical:"));

    let pd_no_mb: Vec<SearchSuggestion> = plastic_death_raw
        .into_iter()
        .filter(|i| !i.item_url_path.starts_with("canonical:"))
        .collect();
    let pd_fallback = run_dropdown("plastic death", pd_no_mb);
    assert_eq!(pd_fallback[0].item_type, ItemType::Album);
    assert_eq!(pd_fallback[0].band_name, "glass beach");
}

#[tokio::test]
#[ignore]
async fn live_search_smoke_test_against_real_providers() {
    use std::time::Duration;
    #[derive(Clone, Copy)]
    enum TopKind {
        Album,
        TrackOrAlbum,
        Artist,
    }
    // (query, expected_top, expected_substring)
    let queries = [
        ("death grips no love deep web", TopKind::Album, "no love deep web"),
        ("morrison hotel", TopKind::Album, "morrison hotel"),
        ("aphex twin", TopKind::Artist, "aphex twin"),
        ("the doors albums", TopKind::Album, "the doors"),
        ("the doors strange", TopKind::TrackOrAlbum, "strange days"),
        ("the beatle white album", TopKind::Album, "beatles"),
        ("daft punk discovery", TopKind::Album, "discovery"),
        ("radiohead in rainbows", TopKind::Album, "in rainbows"),
        ("aphex twin drukqs", TopKind::Album, "drukqs"),
        ("pink floyd wish you were here", TopKind::TrackOrAlbum, "wish you were here"),
        ("nirvana nevermind", TopKind::Album, "nevermind"),
        ("tame impala currents", TopKind::Album, "currents"),
        ("plastic death", TopKind::Album, "plastic death"),
    ];

    for (q, expected_top, exp_substr) in queries {
        let (yt, bc, sc, mb) = tokio::join!(
            super::search_youtube_with_fallback(q),
            super::bandcamp::search(q),
            crate::platforms::soundcloud::search::search(q),
            super::fetch_canonical_album_suggestions(q, None, Platform::Youtube, 6, Duration::from_secs(4)),
        );
        let mut all = Vec::new();
        if let Ok(items) = yt {
            all.extend(items);
        }
        if let Ok(items) = bc {
            all.extend(items);
        }
        if let Ok(items) = sc {
            all.extend(items);
        }
        all.extend(mb);

        let ranked = run_dropdown(q, all);
        println!("\n=== LIVE QUERY: '{q}' ===");
        for (i, item) in ranked.iter().take(6).enumerate() {
            println!(
                "  #{}: {:?} | '{}' by '{}' ({:?}) -> {}",
                i + 1,
                item.item_type,
                item.name,
                item.band_name,
                item.platform,
                item.item_url_path
            );
        }
        let top = ranked.first().expect("live query returned empty");
        match expected_top {
            TopKind::Artist => {
                assert_eq!(
                    top.item_type, ItemType::Artist,
                    "live top wrong type for '{q}': got {:?} '{}' by '{}'",
                    top.item_type, top.name, top.band_name
                );
                assert!(
                    ranked.iter().take(6).any(|i| i.item_type == ItemType::Album && normalize(&i.band_name).contains(exp_substr)),
                    "expected official album by '{exp_substr}' in top 6 for '{q}'"
                );
            }
            TopKind::TrackOrAlbum => {
                assert!(
                    matches!(top.item_type, ItemType::Album | ItemType::Track),
                    "live top wrong type for '{q}': got {:?}",
                    top.item_type
                );
                assert!(
                    ranked.iter().take(5).any(|i| i.item_type == ItemType::Album && normalize(&i.name).contains(exp_substr)),
                    "expected album '{exp_substr}' in top 5 for '{q}'"
                );
            }
            TopKind::Album => {
                assert_eq!(
                    top.item_type, ItemType::Album,
                    "live top wrong type for '{q}': got {:?} '{}' by '{}'",
                    top.item_type, top.name, top.band_name
                );
            }
        }
        assert!(
            normalize(&top.name).contains(exp_substr) || normalize(&top.band_name).contains(exp_substr),
            "live top mismatch for '{q}': got '{}' by '{}'",
            top.name,
            top.band_name
        );
    }
}
