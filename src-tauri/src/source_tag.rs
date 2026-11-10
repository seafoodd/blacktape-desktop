//! Where a downloaded track came from, stored in the file itself.
//!
//! Fields with the same names in every format, so tag editors (Mp3tag, foobar2000, Picard) show them as one
//! field and keep them on save: `BLACKTAPE_SOURCE` (platform), `BLACKTAPE_SOURCE_ID` (id on the platform) and
//! `BLACKTAPE_SOURCE_URL` (track page; the only unique key for Bandcamp/SoundCloud, whose ids may be bare slugs).
//!
//! | format             | stored as                                                        |
//! |--------------------|------------------------------------------------------------------|
//! | MP3                | `TXXX:<name>` frames, plus the url in the standard `WOAS` frame  |
//! | Opus, Ogg, FLAC    | Vorbis comments `<name>`                                         |
//! | M4A                | freeform atoms `----:com.apple.iTunes:<name>`                    |
//!
//! Older versions wrote `blacktape_source:YouTube|id:…` into the comment. That is still read, and
//! [`write_source`] removes it (only that exact marker; any other comment stays).

use crate::types::Platform;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFile};
use lofty::flac::FlacFile;
use lofty::id3::v2::{ExtendedTextFrame, Frame, FrameId, Id3v2Tag, UrlLinkFrame};
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::{OpusFile, SpeexFile, VorbisComments, VorbisFile};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::TextEncoding;
use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::path::Path;

pub const SOURCE_KEY: &str = "BLACKTAPE_SOURCE";
pub const SOURCE_ID_KEY: &str = "BLACKTAPE_SOURCE_ID";
pub const SOURCE_URL_KEY: &str = "BLACKTAPE_SOURCE_URL";
const MP4_MEAN: &str = "com.apple.iTunes";
const LEGACY_PREFIX: &str = "blacktape_source:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub platform: Platform,
    pub id: String,
    /// Page of the track on its platform (http/https only).
    pub url: Option<String>,
}

/// A file read once, with its source if it has one.
pub struct ReadFile {
    pub tagged: TaggedFile,
    pub source: Option<SourceRef>,
    /// The source came from the old comment marker, so the file should be migrated.
    pub legacy: bool,
}

pub fn platform_key(platform: Platform) -> Option<&'static str> {
    match platform {
        Platform::Youtube => Some("youtube"),
        Platform::Bandcamp => Some("bandcamp"),
        Platform::Soundcloud => Some("soundcloud"),
        Platform::Local => None,
    }
}

pub fn parse_platform(value: &str) -> Option<Platform> {
    match value.trim().to_ascii_lowercase().as_str() {
        "youtube" | "youtubemusic" => Some(Platform::Youtube),
        "bandcamp" => Some(Platform::Bandcamp),
        "soundcloud" => Some(Platform::Soundcloud),
        _ => None,
    }
}

fn source_from(platform: Option<&str>, id: Option<&str>) -> Option<SourceRef> {
    let platform = parse_platform(platform?)?;
    let id = id?.trim();
    (!id.is_empty()).then(|| SourceRef {
        platform,
        id: id.to_string(),
        url: None,
    })
}

fn web_url(url: Option<&str>) -> Option<String> {
    url.map(str::trim).filter(|u| is_web_url(u)).map(str::to_string)
}

/// Parses the old `blacktape_source:YouTube|id:abc` comment.
pub fn parse_legacy_comment(comment: &str) -> Option<SourceRef> {
    let rest = comment.trim().strip_prefix(LEGACY_PREFIX)?;
    let mut parts = rest.split('|');
    let platform = parts.next();
    let id = parts.next().and_then(|p| p.trim().strip_prefix("id:"));
    source_from(platform, id)
}

fn is_legacy_marker(comment: &str) -> bool {
    parse_legacy_comment(comment).is_some()
}

// --- per-format access -----------------------------------------------------------------------

fn woas() -> FrameId<'static> {
    FrameId::Valid(Cow::Borrowed("WOAS"))
}

fn id3_source(tag: &Id3v2Tag) -> Option<SourceRef> {
    let mut source = source_from(tag.get_user_text(SOURCE_KEY), tag.get_user_text(SOURCE_ID_KEY))?;
    let woas_url = match tag.get(&woas()) {
        Some(Frame::Url(link)) => Some(link.url()),
        _ => None,
    };
    source.url = web_url(tag.get_user_text(SOURCE_URL_KEY)).or_else(|| web_url(woas_url));
    Some(source)
}

fn id3_legacy(tag: &Id3v2Tag) -> Option<SourceRef> {
    tag.into_iter().find_map(|frame| match frame {
        Frame::Comment(c) => parse_legacy_comment(&c.content),
        _ => None,
    })
}

fn set_id3(tag: &mut Id3v2Tag, source: &SourceRef, key: &str) {
    tag.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        SOURCE_KEY,
        key.to_string(),
    )));
    tag.insert(Frame::UserText(ExtendedTextFrame::new(
        TextEncoding::UTF8,
        SOURCE_ID_KEY,
        source.id.clone(),
    )));
    if let Some(url) = web_url(source.url.as_deref()) {
        tag.insert(Frame::UserText(ExtendedTextFrame::new(
            TextEncoding::UTF8,
            SOURCE_URL_KEY,
            url.clone(),
        )));
        tag.insert(Frame::Url(UrlLinkFrame::new(woas(), url)));
    }
    tag.retain(|frame| !matches!(frame, Frame::Comment(c) if is_legacy_marker(&c.content)));
}

fn vorbis_source(tag: &VorbisComments) -> Option<SourceRef> {
    let mut source = source_from(tag.get(SOURCE_KEY), tag.get(SOURCE_ID_KEY))?;
    source.url = web_url(tag.get(SOURCE_URL_KEY));
    Some(source)
}

fn vorbis_legacy(tag: &VorbisComments) -> Option<SourceRef> {
    tag.get_all("COMMENT").find_map(parse_legacy_comment)
}

fn set_vorbis(tag: &mut VorbisComments, source: &SourceRef, key: &str) {
    tag.insert(SOURCE_KEY.to_string(), key.to_string());
    tag.insert(SOURCE_ID_KEY.to_string(), source.id.clone());
    if let Some(url) = web_url(source.url.as_deref()) {
        tag.insert(SOURCE_URL_KEY.to_string(), url);
    }
    let comments: Vec<String> = tag.remove("COMMENT").collect();
    for comment in comments.into_iter().filter(|c| !is_legacy_marker(c)) {
        tag.push("COMMENT".to_string(), comment);
    }
}

fn mp4_ident(name: &'static str) -> AtomIdent<'static> {
    AtomIdent::Freeform {
        mean: Cow::Borrowed(MP4_MEAN),
        name: Cow::Borrowed(name),
    }
}

fn mp4_text<'a>(tag: &'a Ilst, name: &'static str) -> Option<&'a str> {
    tag.get(&mp4_ident(name))?.data().find_map(|d| match d {
        AtomData::UTF8(s) | AtomData::UTF16(s) => Some(s.as_str()),
        _ => None,
    })
}

const MP4_COMMENT: AtomIdent<'static> = AtomIdent::Fourcc(*b"\xa9cmt");

fn mp4_source(tag: &Ilst) -> Option<SourceRef> {
    let mut source = source_from(mp4_text(tag, SOURCE_KEY), mp4_text(tag, SOURCE_ID_KEY))?;
    source.url = web_url(mp4_text(tag, SOURCE_URL_KEY));
    Some(source)
}

fn mp4_legacy(tag: &Ilst) -> Option<SourceRef> {
    tag.get(&MP4_COMMENT)?.data().find_map(|d| match d {
        AtomData::UTF8(s) | AtomData::UTF16(s) => parse_legacy_comment(s),
        _ => None,
    })
}

fn set_mp4(tag: &mut Ilst, source: &SourceRef, key: &str) {
    tag.replace_atom(Atom::new(mp4_ident(SOURCE_KEY), AtomData::UTF8(key.to_string())));
    tag.replace_atom(Atom::new(
        mp4_ident(SOURCE_ID_KEY),
        AtomData::UTF8(source.id.clone()),
    ));
    if let Some(url) = web_url(source.url.as_deref()) {
        tag.replace_atom(Atom::new(mp4_ident(SOURCE_URL_KEY), AtomData::UTF8(url)));
    }
    let legacy = tag
        .get(&MP4_COMMENT)
        .is_some_and(|atom| atom.data().any(|d| matches!(d, AtomData::UTF8(s) | AtomData::UTF16(s) if is_legacy_marker(s))));
    if legacy {
        let _ = tag.remove(&MP4_COMMENT).count();
    }
}

/// (source, came from the old comment)
fn pick(current: Option<SourceRef>, legacy: impl FnOnce() -> Option<SourceRef>) -> (Option<SourceRef>, bool) {
    match current {
        Some(source) => (Some(source), false),
        None => match legacy() {
            Some(source) => (Some(source), true),
            None => (None, false),
        },
    }
}

// --- public API ------------------------------------------------------------------------------

fn file_type(path: &Path) -> Result<Option<FileType>, String> {
    Ok(Probe::open(path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .file_type())
}

/// Reads a file once: its generic tags for the scan plus its source.
pub fn read_file(path: &Path) -> Result<ReadFile, String> {
    let options = ParseOptions::new();
    let open = || File::open(path).map_err(|e| e.to_string());
    let err = |e: lofty::error::LoftyError| e.to_string();

    let (tagged, (source, legacy)) = match file_type(path)? {
        Some(FileType::Mpeg) => {
            let file = MpegFile::read_from(&mut open()?, options).map_err(err)?;
            let found = file.id3v2().map_or((None, false), |t| pick(id3_source(t), || id3_legacy(t)));
            (TaggedFile::from(file), found)
        }
        Some(FileType::Flac) => {
            let file = FlacFile::read_from(&mut open()?, options).map_err(err)?;
            let found = file
                .vorbis_comments()
                .map_or((None, false), |t| pick(vorbis_source(t), || vorbis_legacy(t)));
            (TaggedFile::from(file), found)
        }
        Some(FileType::Opus) => {
            let file = OpusFile::read_from(&mut open()?, options).map_err(err)?;
            let t = file.vorbis_comments();
            let found = pick(vorbis_source(t), || vorbis_legacy(t));
            (TaggedFile::from(file), found)
        }
        Some(FileType::Vorbis) => {
            let file = VorbisFile::read_from(&mut open()?, options).map_err(err)?;
            let t = file.vorbis_comments();
            let found = pick(vorbis_source(t), || vorbis_legacy(t));
            (TaggedFile::from(file), found)
        }
        Some(FileType::Speex) => {
            let file = SpeexFile::read_from(&mut open()?, options).map_err(err)?;
            let t = file.vorbis_comments();
            let found = pick(vorbis_source(t), || vorbis_legacy(t));
            (TaggedFile::from(file), found)
        }
        Some(FileType::Mp4) => {
            let file = Mp4File::read_from(&mut open()?, options).map_err(err)?;
            let found = file.ilst().map_or((None, false), |t| pick(mp4_source(t), || mp4_legacy(t)));
            (TaggedFile::from(file), found)
        }
        // Other formats (WAV, AIFF, …) are never downloaded; only the old comment can carry a source there.
        _ => {
            let tagged = Probe::open(path)
                .map_err(|e| e.to_string())?
                .read()
                .map_err(err)?;
            let legacy = tagged
                .primary_tag()
                .and_then(|t| t.get_string(ItemKey::Comment))
                .and_then(parse_legacy_comment);
            let legacy_found = legacy.is_some();
            (tagged, (legacy, legacy_found))
        }
    };

    Ok(ReadFile {
        tagged,
        source,
        legacy,
    })
}

/// Reads only the source of a file.
pub fn read_source(path: &Path) -> Result<Option<SourceRef>, String> {
    read_file(path).map(|f| f.source)
}

/// Runs a save through lofty's generic `Tag`, which rewrites the whole tag and drops fields it does not know
/// (ours among them), then puts the source back.
pub fn preserving_source<T>(
    path: &Path,
    save: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let source = read_source(path).ok().flatten();
    let result = save()?;
    if let Some(source) = source {
        write_source(path, &source)?;
    }
    Ok(result)
}

fn is_web_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Writes the source fields (the url only when it is http/https), removing the old comment marker.
/// A source without a url leaves an url already in the file untouched.
/// Returns false for a format that has no place for them.
pub fn write_source(path: &Path, source: &SourceRef) -> Result<bool, String> {
    let Some(key) = platform_key(source.platform) else {
        return Ok(false);
    };
    let options = ParseOptions::new();
    let err = |e: lofty::error::LoftyError| e.to_string();
    let mut handle = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;

    macro_rules! save {
        ($file:expr) => {{
            handle.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            $file.save_to(&mut handle, WriteOptions::default()).map_err(err)?;
        }};
    }

    match file_type(path)? {
        Some(FileType::Mpeg) => {
            let mut file = MpegFile::read_from(&mut handle, options).map_err(err)?;
            if file.id3v2().is_none() {
                file.set_id3v2(Id3v2Tag::new());
            }
            if let Some(tag) = file.id3v2_mut() {
                set_id3(tag, source, key);
            }
            save!(file);
        }
        Some(FileType::Flac) => {
            let mut file = FlacFile::read_from(&mut handle, options).map_err(err)?;
            if file.vorbis_comments().is_none() {
                file.set_vorbis_comments(VorbisComments::new());
            }
            if let Some(tag) = file.vorbis_comments_mut() {
                set_vorbis(tag, source, key);
            }
            save!(file);
        }
        Some(FileType::Opus) => {
            let mut file = OpusFile::read_from(&mut handle, options).map_err(err)?;
            set_vorbis(file.vorbis_comments_mut(), source, key);
            save!(file);
        }
        Some(FileType::Vorbis) => {
            let mut file = VorbisFile::read_from(&mut handle, options).map_err(err)?;
            set_vorbis(file.vorbis_comments_mut(), source, key);
            save!(file);
        }
        Some(FileType::Speex) => {
            let mut file = SpeexFile::read_from(&mut handle, options).map_err(err)?;
            set_vorbis(file.vorbis_comments_mut(), source, key);
            save!(file);
        }
        Some(FileType::Mp4) => {
            let mut file = Mp4File::read_from(&mut handle, options).map_err(err)?;
            if file.ilst().is_none() {
                file.set_ilst(Ilst::new());
            }
            if let Some(tag) = file.ilst_mut() {
                set_mp4(tag, source, key);
            }
            save!(file);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::BasicTags;
    use lofty::tag::Tag;
    use std::path::PathBuf;

    const FORMATS: [&str; 5] = ["mp3", "opus", "m4a", "flac", "ogg"];

    /// Copies a fixture into `dir` so the checked-in file is never written to.
    fn copy_fixture(dir: &tempfile::TempDir, ext: &str) -> PathBuf {
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/audio/silence.{ext}"));
        let to = dir.path().join(format!("track.{ext}"));
        std::fs::copy(&from, &to).unwrap_or_else(|e| panic!("copy {} : {e}", from.display()));
        to
    }

    fn yt(id: &str) -> SourceRef {
        SourceRef {
            platform: Platform::Youtube,
            id: id.into(),
            url: Some(format!("https://www.youtube.com/watch?v={id}")),
        }
    }

    /// Sets the generic comment through lofty, the way another tag editor would.
    fn set_generic_comment(path: &Path, text: &str) {
        let mut tagged = Probe::open(path).unwrap().read().unwrap();
        if tagged.primary_tag().is_none() {
            let tag_type = tagged.file_type().primary_tag_type();
            tagged.insert_tag(Tag::new(tag_type));
        }
        tagged
            .primary_tag_mut()
            .unwrap()
            .insert_text(ItemKey::Comment, text.to_string());
        tagged.save_to_path(path, WriteOptions::default()).unwrap();
    }

    fn generic_comment(path: &Path) -> Option<String> {
        let tagged = Probe::open(path).unwrap().read().unwrap();
        tagged
            .primary_tag()
            .and_then(|t| t.get_string(ItemKey::Comment))
            .map(str::to_string)
    }

    fn generic_title(path: &Path) -> Option<String> {
        let tagged = Probe::open(path).unwrap().read().unwrap();
        tagged
            .primary_tag()
            .or_else(|| tagged.first_tag())
            .and_then(|t| t.get_string(ItemKey::TrackTitle))
            .map(str::to_string)
    }

    #[test]
    fn parse_platform_maps_known_names() {
        assert_eq!(parse_platform("youtube"), Some(Platform::Youtube));
        assert_eq!(parse_platform("YouTube"), Some(Platform::Youtube));
        assert_eq!(parse_platform(" youtubemusic "), Some(Platform::Youtube));
        assert_eq!(parse_platform("bandcamp"), Some(Platform::Bandcamp));
        assert_eq!(parse_platform("SoundCloud"), Some(Platform::Soundcloud));
        assert_eq!(parse_platform("local"), None);
        assert_eq!(parse_platform(""), None);
        assert_eq!(parse_platform("spotify"), None);
    }

    #[test]
    fn platform_key_is_lowercase_name_or_none_for_local() {
        assert_eq!(platform_key(Platform::Local), None);
        assert_eq!(platform_key(Platform::Youtube), Some("youtube"));
        assert_eq!(platform_key(Platform::Bandcamp), Some("bandcamp"));
        assert_eq!(platform_key(Platform::Soundcloud), Some("soundcloud"));
    }

    #[test]
    fn parse_legacy_comment_cases() {
        assert_eq!(
            parse_legacy_comment("blacktape_source:YouTube|id:abc"),
            Some(SourceRef {
                platform: Platform::Youtube,
                id: "abc".into(),
                url: None,
            })
        );
        assert_eq!(
            parse_legacy_comment("blacktape_source:Bandcamp|id: 42 ").map(|s| (s.platform, s.id)),
            Some((Platform::Bandcamp, "42".to_string()))
        );
        assert_eq!(parse_legacy_comment("blacktape_source:Local|id:x"), None);
        assert_eq!(parse_legacy_comment("blacktape_source:YouTube|id:"), None);
        assert_eq!(parse_legacy_comment("blacktape_source:YouTube"), None);
        assert_eq!(parse_legacy_comment("my own comment"), None);
        assert_eq!(parse_legacy_comment(""), None);
    }

    #[test]
    fn write_then_read_roundtrip_per_format() {
        for ext in FORMATS {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            assert_eq!(read_source(&p), Ok(None), "fresh .{ext} should have no source");

            let source = yt("abc123");
            assert_eq!(write_source(&p, &source), Ok(true), "write .{ext}");

            let read = read_source(&p)
                .unwrap()
                .unwrap_or_else(|| panic!("source missing after write .{ext}"));
            assert_eq!(read.platform, Platform::Youtube, ".{ext} platform");
            assert_eq!(read.id, "abc123", ".{ext} id");
            let expected_url = source.url.clone();
            assert_eq!(read.url, expected_url, ".{ext} url");
        }
    }

    #[test]
    fn writing_twice_keeps_latest_id_per_format() {
        for ext in FORMATS {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            assert_eq!(write_source(&p, &yt("first")), Ok(true), "first write .{ext}");
            assert_eq!(write_source(&p, &yt("second")), Ok(true), "second write .{ext}");

            let read = read_source(&p).unwrap().expect("source present");
            assert_eq!(read.id, "second", ".{ext} id after two writes");

            if ext == "opus" {
                let mut file = std::fs::File::open(&p).unwrap();
                let opus = OpusFile::read_from(&mut file, ParseOptions::new()).unwrap();
                assert_eq!(
                    opus.vorbis_comments().get_all(SOURCE_ID_KEY).count(),
                    1,
                    "exactly one {SOURCE_ID_KEY} value in opus"
                );
            }
        }
    }

    #[test]
    fn url_that_is_not_http_is_not_stored_on_mp3() {
        let dir = tempfile::tempdir().unwrap();
        let p = copy_fixture(&dir, "mp3");
        let source = SourceRef {
            platform: Platform::Youtube,
            id: "abc".into(),
            url: Some("javascript:alert(1)".into()),
        };
        assert_eq!(write_source(&p, &source), Ok(true));
        let read = read_source(&p).unwrap().expect("source present");
        assert_eq!(read.id, "abc");
        assert_eq!(read.url, None);
    }

    #[test]
    fn local_platform_writes_nothing() {
        for ext in FORMATS {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            let before = std::fs::read(&p).unwrap();
            let local = SourceRef {
                platform: Platform::Local,
                id: "x".into(),
                url: None,
            };
            assert_eq!(write_source(&p, &local), Ok(false), ".{ext} local write");
            assert_eq!(std::fs::read(&p).unwrap(), before, ".{ext} bytes unchanged");
            assert_eq!(read_source(&p), Ok(None), ".{ext} still no source");
        }
    }

    #[test]
    fn legacy_comment_is_read_then_migrated() {
        for ext in ["mp3", "opus", "m4a"] {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            set_generic_comment(&p, "blacktape_source:YouTube|id:legacy1");

            let read = read_file(&p).unwrap();
            let legacy = read.legacy;
            let source = read.source.unwrap_or_else(|| panic!("legacy source .{ext}"));
            assert_eq!(source.id, "legacy1", ".{ext} legacy id");
            assert_eq!(source.platform, Platform::Youtube, ".{ext} legacy platform");
            assert!(legacy, ".{ext} legacy flag set on read");

            assert_eq!(write_source(&p, &source), Ok(true), "migrate .{ext}");

            let after = read_file(&p).unwrap();
            assert!(!after.legacy, ".{ext} legacy flag cleared after migration");
            assert_eq!(after.source.map(|s| s.id), Some("legacy1".to_string()), ".{ext} id kept");
            assert_eq!(generic_comment(&p), None, ".{ext} marker removed from comment");
        }
    }

    #[test]
    fn user_comment_is_kept_on_write() {
        for ext in ["mp3", "opus"] {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            set_generic_comment(&p, "my note");

            assert_eq!(write_source(&p, &yt("new1")), Ok(true), "write .{ext}");

            assert_eq!(generic_comment(&p).as_deref(), Some("my note"), ".{ext} user comment kept");
            let read = read_file(&p).unwrap();
            assert!(!read.legacy, ".{ext} not legacy");
            assert_eq!(read.source.map(|s| s.id), Some("new1".to_string()), ".{ext} new source");
        }
    }

    #[test]
    fn source_survives_basic_tag_write() {
        for ext in ["mp3", "opus"] {
            let dir = tempfile::tempdir().unwrap();
            let p = copy_fixture(&dir, ext);
            assert_eq!(write_source(&p, &yt("keep1")), Ok(true), "write .{ext}");
            let before = read_source(&p).unwrap().expect("source present");

            let artists = vec!["A".to_string()];
            crate::tags::write_basic_tags(
                &p,
                &BasicTags {
                    title: "New",
                    artists: &artists,
                    album: "Al",
                    album_artist: "A",
                    track_number: Some(1),
                    genre: None,
                    release_year: None,
                },
            )
            .unwrap_or_else(|e| panic!(".{ext} basic tag write failed: {e}"));

            let after = read_source(&p).unwrap().expect("source still present");
            assert_eq!(after, before, ".{ext} source and url preserved");
            assert_eq!(generic_title(&p).as_deref(), Some("New"), ".{ext} title written");
        }
    }

    #[test]
    fn read_file_on_missing_path_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.mp3");
        assert!(read_file(&missing).is_err());
        assert!(read_source(&missing).is_err());
    }
}
