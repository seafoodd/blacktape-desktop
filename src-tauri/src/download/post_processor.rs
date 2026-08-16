use crate::download::types::TrackDownload;
use crate::types::Platform;
use crate::utils::{determine_quality, make_canonical_slug, sanitize_fs};
use crate::Song;
use html_escape::decode_html_entities;
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, Tag, TagExt};
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::str::FromStr;

pub fn process_and_move_track(
    source_file: &Path,
    track: &TrackDownload,
    target_album_dir: &Path,
    cover_path: Option<&Path>,
    covers_dir: &Path,
    platform: Platform,
) -> Result<Song, String> {
    if !source_file.exists() {
        return Err(format!("Source track stream missing: {:?}", source_file));
    }

    let title = decode_html_entities(&track.title).to_string();
    let artists: Vec<String> = track
        .artists
        .iter()
        .map(|a| decode_html_entities(a).to_string())
        .collect();
    let album_artist = decode_html_entities(&track.album_artist).to_string();
    println!("decoded html: {}, {}", album_artist, &track.album_artist);
    let album = decode_html_entities(&track.album).to_string();

    let ext = source_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp3");

    if let Err(err) = apply_metadata_tags(
        source_file,
        &title,
        &artists,
        &album,
        &album_artist,
        track,
        cover_path,
        platform,
    ) {
        eprintln!("[Metadata Tagging Warning] Issue writing ID3 headers: {err}");
    }

    let clean_title = sanitize_fs(&track.title);
    let final_name = match track.track_number {
        Some(num) => format!("{:02} - {}.{}", num, clean_title, ext),
        None => format!("{}.{}", clean_title, ext),
    };

    let destination_path = target_album_dir.join(final_name);
    fs::create_dir_all(target_album_dir)
        .map_err(|e| format!("Failed creating target album dir: {e}"))?;

    fs::rename(source_file, &destination_path)
        .map_err(|e| format!("Failed moving file into library: {e}"))?;

    let tagged_file = Probe::open(&destination_path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let duration_ms = tagged_file.properties().duration().as_millis() as u64;
    let final_cover = resolve_cover_path(cover_path, &tagged_file, track, covers_dir);

    Ok(Song {
        id: None,
        path: destination_path.to_string_lossy().to_string(),
        title,
        artists,
        album_artist,
        album,
        duration_ms,
        track_number: track.track_number,
        genre: track.genres.as_ref().and_then(|g| g.first().cloned()),
        release_year: track.release_year,
        cover_url: final_cover,
        external_cover_url: None,
        lyrics: None,
        lyrics_source: None,
        source: platform,
        source_url: Some(track.url.clone()),
        source_item_id: track.source_item_id.clone(),
        canonical_track_slug: make_canonical_slug(&track.album_artist, &track.title),
        canonical_album_slug: make_canonical_slug(&track.album_artist, &track.album),
        quality_tier: determine_quality(ext, &tagged_file),
    })
}

fn apply_metadata_tags(
    file_path: &Path,
    title: &str,
    artists: &[String],
    album: &str,
    album_artist: &str,
    track: &TrackDownload,
    cover_path: Option<&Path>,
    source: Platform,
) -> Result<(), String> {
    let mut tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(t) => t,
        None => {
            let primary_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(primary_type));
            tagged_file.primary_tag_mut().unwrap()
        }
    };

    tag.set_title(title.to_string());
    tag.set_artist(artists.join(", "));
    tag.set_album(album.to_string());
    tag.insert_text(lofty::tag::ItemKey::AlbumArtist, album_artist.to_string());

    if let Some(ref source_id) = track.source_item_id {
        let source_str = match source {
            Platform::Youtube => "YouTube",
            Platform::Bandcamp => "Bandcamp",
            Platform::Local => "Local",
        };
        tag.insert_text(
            lofty::tag::ItemKey::Comment,
            format!("blacktape_source:{source_str}|id:{source_id}"),
        );
    }

    if let Some(year) = track.release_year {
        if let Ok(ts) = Timestamp::from_str(&year.to_string()) {
            tag.set_date(ts);
        }
    }

    if let Some(num) = track.track_number {
        tag.set_track(num as u32);
    }

    if let Some(path) = cover_path.filter(|p| p.exists()) {
        if let Ok(mut img) = File::open(path) {
            if let Ok(mut pic) = Picture::from_reader(&mut img) {
                pic.set_pic_type(PictureType::CoverFront);
                tag.push_picture(pic);
            }
        }
    }

    tag.save_to_path(file_path, WriteOptions::default())
        .map_err(|e| e.to_string())
}

fn resolve_cover_path(
    explicit_cover: Option<&Path>,
    tagged_file: &lofty::file::TaggedFile,
    track: &TrackDownload,
    covers_dir: &Path,
) -> Option<String> {
    if let Some(path) = explicit_cover {
        return Some(path.to_string_lossy().to_string());
    }

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    let pic = tag.pictures().first()?;

    let hash_input = format!("{}{}", track.artists.join(", "), track.album);
    let mut hasher = DefaultHasher::new();
    hash_input.hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    let ext = if pic
        .mime_type()
        .map_or(false, |m| m.as_str().contains("png"))
    {
        "png"
    } else {
        "jpg"
    };

    let target_file = covers_dir.join(format!("{hash}.{ext}"));
    if !target_file.exists() {
        let _ = fs::write(&target_file, pic.data());
    }

    Some(target_file.to_string_lossy().to_string())
}
