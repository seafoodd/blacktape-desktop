use crate::trash::TRASH_DIR_NAME;
use crate::types::{Platform, QualityTier, Song};
use crate::utils::{determine_quality, make_canonical_slug};
use dashmap::{DashMap, DashSet};
use jwalk::WalkDir;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64 as hash64;

/// Audio files under `dir`, skipping every directory named like the trash folder (D18).
pub fn collect_audio_paths(dir: &str) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .process_read_dir(|_depth, _path, _state, children| {
            // Pruned before descending, so trashed files are never even opened.
            children.retain(|entry| match entry {
                Ok(e) => {
                    !(e.file_type().is_dir() && e.file_name() == OsStr::new(TRASH_DIR_NAME))
                }
                Err(_) => true,
            });
        })
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            matches!(
                ext,
                "mp3" | "flac" | "ogg" | "wav" | "m4a" | "aiff" | "opus"
            )
        })
        .collect()
}

pub fn scan_music_dir(dir: String, covers_dir: &PathBuf) -> Vec<Song> {
    if !covers_dir.exists() {
        let _ = fs::create_dir_all(covers_dir);
    }

    let dir_covers: Arc<DashMap<PathBuf, Option<String>>> = Arc::new(DashMap::new());
    let written_hashes: Arc<DashSet<String>> = Arc::new(DashSet::new());

    let entries: Vec<PathBuf> = collect_audio_paths(&dir);

    println!(
        "[Scanner] Found {} matching files by extension on disk. Inspecting containers...",
        entries.len()
    );

    entries
        .into_par_iter()
        .filter_map(|path| {
            // One read gives the generic tags and the source fields (format-specific, see source_tag).
            let read = match crate::source_tag::read_file(&path) {
                Ok(read) => read,
                Err(err) => {
                    eprintln!(
                        "[Scanner Error] Reading tags failed for extension '{:?}' at {:?}: {}",
                        path.extension().unwrap_or_default(),
                        path,
                        err
                    );
                    return None;
                }
            };
            let tagged_file = read.tagged;
            let file_source = read.source;

            // Files from older versions keep the source in the comment: move it to its own fields once.
            if read.legacy {
                if let Some(source) = &file_source {
                    if let Err(err) = crate::source_tag::write_source(&path, source) {
                        eprintln!("[Scanner] Could not move the source out of the comment of {:?}: {}", path, err);
                    }
                }
            }

            // Optional metrics tracking print
            println!(
                "[Scanner Success] Parsed {:?} - Format: {:?} | Bitrate: {:?} kbps",
                path.file_name().unwrap_or_default(),
                tagged_file.file_type(),
                tagged_file.properties().audio_bitrate()
            );

            let tag = tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag());

            let title = tag
                .and_then(|t| t.title().map(|s| s.to_string()))
                .unwrap_or_else(|| {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                        .to_string()
                });

            let raw_artist = tag
                .and_then(|t| t.artist().map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown Artist".to_string());

            let artists = parse_artists(&raw_artist);

            let album_artist = tag
                .and_then(|t| t.get_string(ItemKey::AlbumArtist).map(|s| s.to_string()))
                .unwrap_or_else(|| artists.first().cloned().unwrap_or_else(|| "Unknown Artist".to_string()));

            let album = tag
                .and_then(|t| t.album().map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown Album".to_string());

            let parent = path.parent().map(|p| p.to_path_buf());
            let mut final_cover = None;

            if let Some(p) = &parent {
                let entry = dir_covers.entry(p.clone()).or_insert_with(|| {
                    [
                        "cover.jpg",
                        "cover.png",
                        "folder.jpg",
                        "front.jpg",
                        "front.png",
                    ]
                        .iter()
                        .map(|name| p.join(name))
                        .find(|file| file.exists())
                        .map(|file| file.to_string_lossy().to_string())
                });
                final_cover = entry.value().clone();
            }

            if final_cover.is_none() {
                if let Some(pic) = tag.and_then(|t| t.pictures().first()) {
                    let hash_input = format!("{raw_artist}{album}");
                    let hash = format!("{:016x}", hash64(hash_input.as_bytes()));
                    let ext = if pic
                        .mime_type()
                        .map_or(false, |m| m.as_str().contains("png"))
                    {
                        "png"
                    } else {
                        "jpg"
                    };
                    let full_path = covers_dir.join(format!("{hash}.{ext}"));

                    if written_hashes.insert(hash) && !full_path.exists() {
                        let _ = fs::write(&full_path, pic.data());
                    }
                    final_cover = Some(full_path.to_string_lossy().to_string());
                }
            }

            let actual_ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let canonical_track_slug = make_canonical_slug(&album_artist, &title);
            let canonical_album_slug = make_canonical_slug(&album_artist, &album);
            let quality_tier = determine_quality(actual_ext, &tagged_file);

            let (source, source_url, source_item_id) = match file_source {
                Some(found) => (found.platform, found.url, Some(found.id)),
                None => (Platform::Local, None, None),
            };

            Some(Song {
                id: None,
                path: path.to_string_lossy().to_string(),
                title,
                artists,
                album_artist,
                album,
                duration_ms: tagged_file.properties().duration().as_millis() as u64,
                track_number: tag.and_then(|t| t.track()).map(|n| n as i32),
                genre: tag.and_then(|t| t.genre()).map(|g| g.to_string()),
                release_year: tag.and_then(|t| t.date()).map(|d| d.year as i32),
                cover_url: final_cover,
                external_cover_url: None,
                lyrics: None,
                lyrics_source: None,

                source,
                source_url,
                source_item_id,
                canonical_track_slug,
                canonical_album_slug,
                quality_tier,
            })
        })
        .collect()
}

pub fn old_scan_music_dir(dir: String, covers_dir: &PathBuf) -> Vec<Song> {
    let mut songs = Vec::new();

    if !covers_dir.exists() {
        fs::create_dir_all(covers_dir).ok();
    }

    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if !["mp3", "flac", "ogg", "wav", "m4a", "aiff"].contains(&ext) {
            continue;
        }

        let tagged_file = match Probe::open(&path).and_then(Probe::read) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };

        let tag = tagged_file.primary_tag().or(tagged_file.first_tag());

        let title = tag
            .and_then(|t| t.title().map(|s| s.to_string()))
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown Track")
                    .to_string()
            });

        let artist = tag
            .and_then(|t| t.artist().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Artist".to_string());

        let album = tag
            .and_then(|t| t.album().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown Album".to_string());

        let mut cover_url = None;

        if let Some(parent) = path.parent() {
            let common_names = [
                "cover.jpg",
                "cover.png",
                "folder.jpg",
                "front.jpg",
                "front.png",
            ];

            for name in common_names {
                let potential_cover = parent.join(name);
                if potential_cover.exists() {
                    cover_url = Some(potential_cover.to_string_lossy().to_string());
                    break;
                }
            }
        }

        if cover_url.is_none() {
            if let Some(t) = tag {
                if let Some(pic) = t.pictures().first() {
                    let data = pic.data();
                    let mime = pic
                        .mime_type()
                        .map(lofty::picture::MimeType::as_str)
                        .unwrap_or("image/jpeg");
                    let album_key = format!("{artist}{album}");

                    let mut hasher = Sha256::new();
                    hasher.update(album_key.as_bytes());
                    let hash_result = hasher.finalize();
                    let hash = hash_result
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>();

                    let pic_ext = if mime.contains("png") { "png" } else { "jpg" };
                    let filename = format!("{hash}.{pic_ext}");
                    let full_path = covers_dir.join(&filename);

                    if !full_path.exists() {
                        let _ = fs::write(&full_path, data);
                    }
                    cover_url = Some(full_path.to_string_lossy().to_string());
                }
            }
        }

        let song = Song {
            id: None,
            path: path.to_string_lossy().to_string(),
            title,
            artists: vec![],
            album,
            duration_ms: tagged_file.properties().duration().as_millis() as u64,
            track_number: tag.and_then(|t| t.track()).map(|n| n as i32),
            genre: tag.and_then(|t| t.genre()).map(|g| g.to_string()),
            release_year: tag.and_then(|t| t.date()).map(|d| d.year as i32),
            cover_url,
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
        };

        songs.push(song);
    }

    songs
}

pub fn get_song_from_path(path: &str) -> Option<Song> {
    println!("get song from path");
    let tagged_file = match Probe::open(path).and_then(|p| p.read()) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to read tags for {path}: {e}");
            return None;
        }
    };

    let tag = tagged_file.primary_tag();

    let raw_artist = tag
        .and_then(|t| t.artist().map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown Artist".into());

    let artists = parse_artists(&raw_artist);

    let album_artist = tag
        .and_then(|t| t.get_string(ItemKey::AlbumArtist).map(|s| s.to_string()))
        .unwrap_or_else(|| {
            artists
                .first()
                .cloned()
                .unwrap_or_else(|| "Unknown Artist".to_string())
        });

    let title = tag
        .and_then(|t| t.title().map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown Title".into());

    let album = tag
        .and_then(|t| t.album().map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown Album".into());

    let path_obj = Path::new(path);
    let actual_ext = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");

    let canonical_track_slug = make_canonical_slug(&album_artist, &title);
    let canonical_album_slug = make_canonical_slug(&album_artist, &album);
    let quality_tier = determine_quality(actual_ext, &tagged_file);

    let song = Song {
        id: None,
        path: path.to_string(),
        title,
        artists,
        album_artist,
        album,
        duration_ms: tagged_file.properties().duration().as_millis() as u64,
        track_number: tag.and_then(|t| t.track()).map(|n| n as i32),
        genre: tag.and_then(|t| t.genre()).map(|g| g.to_string()),
        release_year: tag.and_then(|t| t.date()).map(|d| d.year as i32),
        cover_url: None,
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
        source: Platform::Local,
        source_url: None,
        source_item_id: None,
        canonical_track_slug,
        canonical_album_slug,
        quality_tier,
    };

    Some(song)
}

/// Files whose tags name a platform but no track url, while the library still knows the url from the download
/// (files downloaded before the url was stored in tags). Pairs are (file path, full source to write).
pub fn source_urls_to_backfill(scanned: &[Song], stored: &[Song]) -> Vec<(PathBuf, crate::source_tag::SourceRef)> {
    let stored_by_path: std::collections::HashMap<&str, &Song> =
        stored.iter().map(|s| (s.path.as_str(), s)).collect();
    scanned
        .iter()
        .filter(|song| song.source != Platform::Local && song.source_url.is_none())
        .filter_map(|song| {
            let known = stored_by_path.get(song.path.as_str())?;
            if known.source != song.source {
                return None;
            }
            let url = known
                .source_url
                .as_deref()
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
            let id = song.source_item_id.clone().or_else(|| known.source_item_id.clone())?;
            Some((
                PathBuf::from(&song.path),
                crate::source_tag::SourceRef {
                    platform: song.source,
                    id,
                    url: Some(url.to_string()),
                },
            ))
        })
        .collect()
}

/// Splits raw metadata strings on standard multi-artist delimiters
fn parse_artists(raw_artist: &str) -> Vec<String> {
    raw_artist
        .split(|c| c == ',' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn collect_audio_paths_skips_trash_folder_at_any_depth() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("keep.mp3"));
        touch(&root.join("Artist").join("Album").join("song.flac"));
        touch(&root.join(TRASH_DIR_NAME).join("gone.mp3"));
        touch(
            &root
                .join("Artist")
                .join(TRASH_DIR_NAME)
                .join("nested.mp3"),
        );
        touch(&root.join("Artist").join("notes.txt"));

        let mut found: Vec<String> = collect_audio_paths(&root.to_string_lossy())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();

        assert_eq!(found, vec!["keep.mp3".to_string(), "song.flac".to_string()]);
    }

    fn backfill_song(
        path: &str,
        source: Platform,
        source_url: Option<&str>,
        source_item_id: Option<&str>,
    ) -> Song {
        Song {
            id: None,
            path: path.to_string(),
            title: "Track".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_ms: 1000,
            track_number: Some(1),
            genre: None,
            release_year: None,
            cover_url: None,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source,
            source_url: source_url.map(str::to_string),
            source_item_id: source_item_id.map(str::to_string),
            canonical_track_slug: "artist-track".to_string(),
            canonical_album_slug: "artist-album".to_string(),
            quality_tier: QualityTier::Standard,
        }
    }

    const BANDCAMP_URL: &str = "https://x.bandcamp.com/track/a";

    #[test]
    fn backfill_fills_missing_url_from_matching_stored_row() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, Some("a"))];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some(BANDCAMP_URL),
            Some("a"),
        )];

        let pairs = source_urls_to_backfill(&scanned, &stored);

        assert_eq!(pairs.len(), 1);
        let (path, source) = &pairs[0];
        assert_eq!(path, &PathBuf::from("/m/a.mp3"));
        assert_eq!(source.platform, Platform::Bandcamp);
        assert_eq!(source.id, "a");
        assert_eq!(source.url.as_deref(), Some(BANDCAMP_URL));
    }

    #[test]
    fn backfill_skips_song_that_already_has_url() {
        let scanned = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("https://x.bandcamp.com/track/other"),
            Some("a"),
        )];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some(BANDCAMP_URL),
            Some("a"),
        )];

        assert!(source_urls_to_backfill(&scanned, &stored).is_empty());
    }

    #[test]
    fn backfill_skips_local_songs() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Local, None, None)];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Local,
            Some(BANDCAMP_URL),
            Some("a"),
        )];

        assert!(source_urls_to_backfill(&scanned, &stored).is_empty());
    }

    #[test]
    fn backfill_skips_when_stored_platform_differs() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, Some("a"))];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Youtube,
            Some("https://www.youtube.com/watch?v=a"),
            Some("a"),
        )];

        assert!(source_urls_to_backfill(&scanned, &stored).is_empty());
    }

    #[test]
    fn backfill_skips_stored_url_that_is_missing_or_not_http() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, Some("a"))];

        let no_url = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, Some("a"))];
        assert!(source_urls_to_backfill(&scanned, &no_url).is_empty());

        let js_url = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some("javascript:x"),
            Some("a"),
        )];
        assert!(source_urls_to_backfill(&scanned, &js_url).is_empty());
    }

    #[test]
    fn backfill_skips_when_no_stored_row_has_the_path() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, Some("a"))];
        let stored = vec![backfill_song(
            "/m/other.mp3",
            Platform::Bandcamp,
            Some(BANDCAMP_URL),
            Some("a"),
        )];

        assert!(source_urls_to_backfill(&scanned, &stored).is_empty());
    }

    #[test]
    fn backfill_uses_stored_id_when_scanned_id_missing() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, None)];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some(BANDCAMP_URL),
            Some("sid"),
        )];

        let pairs = source_urls_to_backfill(&scanned, &stored);

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1.id, "sid");
    }

    #[test]
    fn backfill_skips_when_no_id_on_either_side() {
        let scanned = vec![backfill_song("/m/a.mp3", Platform::Bandcamp, None, None)];
        let stored = vec![backfill_song(
            "/m/a.mp3",
            Platform::Bandcamp,
            Some(BANDCAMP_URL),
            None,
        )];

        assert!(source_urls_to_backfill(&scanned, &stored).is_empty());
    }
}
