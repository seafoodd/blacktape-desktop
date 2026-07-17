use crate::types::{Platform, QualityTier, Song};
use crate::utils::{determine_quality, make_canonical_slug};
use dashmap::{DashMap, DashSet};
use jwalk::WalkDir;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_64 as hash64;

pub fn scan_music_dir(dir: String, covers_dir: &PathBuf) -> Vec<Song> {
    if !covers_dir.exists() {
        let _ = fs::create_dir_all(covers_dir);
    }

    let dir_covers: Arc<DashMap<PathBuf, Option<String>>> = Arc::new(DashMap::new());
    let written_hashes: Arc<DashSet<String>> = Arc::new(DashSet::new());

    let entries: Vec<PathBuf> = WalkDir::new(&dir)
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
        .collect();

    println!(
        "[Scanner] Found {} matching files by extension on disk. Inspecting containers...",
        entries.len()
    );

    entries
        .into_par_iter()
        .filter_map(|path| {
            // 1. Break open the Probe steps to explicitly log parsing or IO errors
            let probe = match Probe::open(&path) {
                Ok(p) => p,
                Err(err) => {
                    eprintln!("[Scanner Error] IO/Probe failed to open file structure at {:?}: {}", path, err);
                    return None;
                }
            };

            let tagged_file = match probe.read() {
                Ok(tf) => tf,
                Err(err) => {
                    eprintln!(
                        "[Scanner Error] Lofty metadata stream reading failed for extension '{:?}' at {:?}: {}",
                        path.extension().unwrap_or_default(),
                        path,
                        err
                    );
                    return None;
                }
            };

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

            let mut source = Platform::Local;
            let mut source_item_id = None;

            if let Some(t) = tag {
                if let Some(comment_text) = t.get_string(ItemKey::Comment) {
                    if comment_text.starts_with("blacktape_source:") {
                        let parts: Vec<&str> = comment_text.split('|').collect();

                        if let Some(source_part) = parts.first() {
                            let platform_str = source_part.replace("blacktape_source:", "");
                            source = match platform_str.to_lowercase().as_str() {
                                "youtube" | "youtubemusic" => Platform::Youtube,
                                "bandcamp" => Platform::Bandcamp,
                                _ => Platform::Local,
                            };
                        }

                        if source != Platform::Local {
                            if let Some(id_part) = parts.get(1) {
                                source_item_id = Some(id_part.replace("id:", "").trim().to_string());
                            }
                        }
                    }
                }
            }

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
                source_url: None,
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

/// Splits raw metadata strings on standard multi-artist delimiters
fn parse_artists(raw_artist: &str) -> Vec<String> {
    raw_artist
        .split(|c| c == ',' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
