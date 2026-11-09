//! Live SoundCloud checks (network). Run manually:
//! `cargo test --test soundcloud -- --ignored --nocapture --test-threads=1`
//!
//! The download test needs the app's yt-dlp binary (`%LOCALAPPDATA%\dev.seafood.blacktape\yt-dlp.exe`,
//! or set `BT_YTDLP`) and ffmpeg on PATH. It writes into `BT_LIVE_LIB_DIR` (default: a temp
//! dir), never into the real `~/blacktape-lib` or the app database.

use blacktape_desktop_lib::db::db::Database;
use blacktape_desktop_lib::download::album::download_album_cover;
use blacktape_desktop_lib::download::post_processor::process_and_move_track;
use blacktape_desktop_lib::download::soundcloud::{parse_album_with_skipped, parse_track};
use blacktape_desktop_lib::download::ytdlp::soundcloud_format_args;
use blacktape_desktop_lib::download::TrackDownload;
use blacktape_desktop_lib::soundcloud::{api_get, ScCollection, ScTrack};
use blacktape_desktop_lib::types::{Platform, Song};
use blacktape_desktop_lib::utils::sanitize_fs;
use std::path::{Path, PathBuf};

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

#[tokio::test]
#[ignore]
async fn live_parse_go_plus_track_is_refused() {
    let err = parse_track("https://soundcloud.com/daftpunkofficialmusic/around-the-world")
        .await
        .expect_err("a Go+ preview must not be downloadable");
    println!("{err}");
    assert!(err.contains("30-second preview"), "{err}");
}

fn ytdlp_binary() -> PathBuf {
    if let Ok(p) = std::env::var("BT_YTDLP") {
        return PathBuf::from(p);
    }
    let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
    PathBuf::from(local).join("dev.seafood.blacktape").join("yt-dlp.exe")
}

/// Same base arguments as `ytdlp::download_track_internal`, plus the shared SoundCloud ones.
fn run_ytdlp(url: &str, temp_dir: &Path, index: usize) -> PathBuf {
    let template = temp_dir.join(format!("{index}.%(ext)s"));
    let mut args: Vec<String> = [
        "--ignore-errors",
        "--no-check-certificates",
        "--no-playlist",
        "--concurrent-fragments",
        "10",
        "--http-chunk-size",
        "10M",
        "-o",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(template.to_string_lossy().to_string());
    args.extend(soundcloud_format_args());
    args.push("--newline".into());
    args.push(url.into());

    let out = std::process::Command::new(ytdlp_binary())
        .args(&args)
        .output()
        .expect("yt-dlp failed to start");
    assert!(
        out.status.success(),
        "yt-dlp failed for {url}:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_dir(temp_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_stem().and_then(|s| s.to_str()) == Some(&index.to_string())
                && !matches!(p.extension().and_then(|e| e.to_str()), Some("part" | "ytdl"))
        })
        .unwrap_or_else(|| panic!("no output file for {url}"))
}

async fn library_db() -> Database {
    let db = Database::new(":memory:").await;
    sqlx::query(
        "CREATE TABLE songs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            path TEXT UNIQUE NOT NULL,
            title TEXT NOT NULL,
            artists TEXT NOT NULL DEFAULT '[]',
            album_artist TEXT NOT NULL DEFAULT 'Unknown Artist',
            album TEXT NOT NULL,
            track_number INTEGER,
            duration_ms INTEGER NOT NULL,
            cover_url TEXT,
            external_cover_url TEXT,
            genre TEXT,
            release_year INTEGER,
            lyrics TEXT,
            lyrics_source TEXT,
            source TEXT NOT NULL DEFAULT 'local',
            source_url TEXT,
            source_item_id TEXT,
            canonical_track_slug TEXT NOT NULL DEFAULT '',
            canonical_album_slug TEXT NOT NULL DEFAULT '',
            quality_tier TEXT NOT NULL DEFAULT 'standard'
        );",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    db
}

fn import(
    tracks: &[TrackDownload],
    target_dir: &Path,
    temp_dir: &Path,
    covers_dir: &Path,
    cover: Option<&Path>,
) -> Vec<Song> {
    tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let file = run_ytdlp(&t.url, temp_dir, i + 1);
            println!("downloaded {} -> {:?}", t.title, file);
            process_and_move_track(&file, t, target_dir, cover, covers_dir, Platform::Soundcloud)
                .expect("post-processing failed")
        })
        .collect()
}

#[tokio::test]
#[ignore]
async fn live_download_track_and_set_into_library() {
    let lib = std::env::var("BT_LIVE_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("blacktape-sc-live"));
    let temp = lib.join(".temp");
    let covers = lib.join("covers");
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::create_dir_all(&covers).unwrap();
    println!("library dir: {lib:?}");
    let db = library_db().await;

    // Single free full-length track.
    let (track, cover_url) = parse_track("https://soundcloud.com/forss/flickermood")
        .await
        .expect("parse_track failed");
    let single_dir = lib.join(sanitize_fs(&track.album_artist)).join("Singles");
    std::fs::create_dir_all(&single_dir).unwrap();
    let cover = match cover_url {
        Some(u) => download_album_cover(&u, &single_dir)
            .await
            .ok()
            .map(|_| single_dir.join("cover.jpg")),
        None => None,
    };
    let single = import(&[track], &single_dir, &temp.join("single"), &covers, cover.as_deref());

    // Short set: 6 tracks, of which 3 are Go+ previews and must be skipped.
    let (album, skipped) =
        parse_album_with_skipped("https://soundcloud.com/the-concept-band/sets/the-royal-concept-ep")
            .await
            .expect("parse_album failed");
    assert_eq!(skipped.len(), 3, "{skipped:?}");
    let album_dir = lib
        .join(sanitize_fs(&album.album_artist))
        .join(sanitize_fs(&album.title));
    std::fs::create_dir_all(&album_dir).unwrap();
    let album_cover = match &album.external_cover_url {
        Some(u) => download_album_cover(u, &album_dir)
            .await
            .ok()
            .map(|_| album_dir.join("cover.jpg")),
        None => None,
    };
    let set_temp = temp.join("set");
    std::fs::create_dir_all(&set_temp).unwrap();
    std::fs::create_dir_all(temp.join("single")).ok();
    let set = import(&album.tracks, &album_dir, &set_temp, &covers, album_cover.as_deref());

    let all: Vec<Song> = single.into_iter().chain(set).collect();
    for s in &all {
        println!(
            "{} | {} | {} | #{:?} | {} ms | {:?} | {}",
            s.title,
            s.artists.join(", "),
            s.album,
            s.track_number,
            s.duration_ms,
            s.quality_tier,
            s.path
        );
        // Full length, not a 30 s preview.
        assert!(s.duration_ms > 120_000, "{} is only {} ms", s.title, s.duration_ms);
        assert_eq!(s.source, Platform::Soundcloud);
        assert!(s.source_item_id.is_some());
        assert!(Path::new(&s.path).exists());
    }
    assert!(all[0].duration_ms.abs_diff(213_890) < 2_000, "{}", all[0].duration_ms);

    db.insert_songs(all.clone()).await.expect("db insert failed");
    let stored = db.get_all_songs().await.unwrap();
    assert_eq!(stored.len(), all.len());
    assert!(stored.iter().all(|s| s.source == Platform::Soundcloud));
}
