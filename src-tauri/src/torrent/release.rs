use serde::Serialize;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReleaseLayout {
    NonAudio,
    SingleTrack,
    StandardAlbum,
    MultiDiscAlbum,
    CueImageAlbum,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct InspectedRelease {
    pub info_hash: String,
    pub name: String,
    pub layout: ReleaseLayout,
    pub total_size_bytes: u64,
    pub total_file_count: usize,
    pub tracks: Vec<InspectedTrack>,
    pub cover_art: Vec<InspectedFile>,
    pub cue_sheets: Vec<InspectedFile>,
    pub rip_logs: Vec<InspectedFile>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct InspectedTrack {
    pub file_index: usize,
    pub file_path: String,
    pub title: String,
    pub codec: &'static str,
    pub size_bytes: u64,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub folder: Option<String>,
    pub is_sample: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct InspectedFile {
    pub file_index: usize,
    pub file_path: String,
    pub size_bytes: u64,
}

pub(super) struct RawFile {
    pub index: usize,
    pub components: Vec<String>,
    pub path: String,
    pub length: u64,
    pub padding: bool,
}

pub(super) fn analyze_release(
    info_hash: String,
    name: String,
    total_size_bytes: u64,
    files: Vec<RawFile>,
) -> InspectedRelease {
    let mut release = InspectedRelease {
        info_hash,
        name,
        layout: ReleaseLayout::NonAudio,
        total_size_bytes,
        total_file_count: files.len(),
        tracks: Vec::new(),
        cover_art: Vec::new(),
        cue_sheets: Vec::new(),
        rip_logs: Vec::new(),
    };
    for file in files.into_iter().filter(|file| !file.padding) {
        let Some((filename, folders)) = file.components.split_last() else {
            continue;
        };
        if filename.starts_with("._") {
            continue;
        }
        let Some((stem, extension)) = filename.rsplit_once('.') else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if let Some(codec) = audio_codec(&extension) {
            let (disc_number, track_number, title) = track_title(folders, stem);
            release.tracks.push(InspectedTrack {
                file_index: file.index,
                file_path: file.path,
                title: title.to_owned(),
                codec,
                size_bytes: file.length,
                disc_number,
                track_number,
                folder: (!folders.is_empty()).then(|| folders.join("/")),
                is_sample: folders.iter().any(|folder| {
                    matches!(
                        folder.to_ascii_lowercase().as_str(),
                        "sample" | "samples" | "preview" | "previews"
                    )
                }),
            });
        } else {
            let bucket = match extension.as_str() {
                "jpg" | "jpeg" | "png" | "webp" | "gif" => &mut release.cover_art,
                "cue" => &mut release.cue_sheets,
                "log" => &mut release.rip_logs,
                _ => continue,
            };
            bucket.push(InspectedFile {
                file_index: file.index,
                file_path: file.path,
                size_bytes: file.length,
            });
        }
    }

    release.tracks.sort_by(|left, right| {
        left.disc_number
            .unwrap_or(1)
            .cmp(&right.disc_number.unwrap_or(1))
            .then_with(|| {
                left.track_number
                    .unwrap_or(u32::MAX)
                    .cmp(&right.track_number.unwrap_or(u32::MAX))
            })
            .then_with(|| left.file_path.cmp(&right.file_path))
            .then_with(|| left.file_index.cmp(&right.file_index))
    });
    let tracks: Vec<_> = release
        .tracks
        .iter()
        .filter(|track| !track.is_sample)
        .collect();
    let discs: HashSet<_> = tracks
        .iter()
        .filter_map(|track| track.disc_number)
        .collect();
    release.layout = match tracks.len() {
        0 => ReleaseLayout::NonAudio,
        1 if release.cue_sheets.iter().any(|cue| {
            let parent = cue.file_path.rsplit_once('/').map(|(parent, _)| parent);
            tracks[0].folder.as_deref() == parent
        }) =>
        {
            ReleaseLayout::CueImageAlbum
        }
        1 => ReleaseLayout::SingleTrack,
        _ if discs.len() > 1 => ReleaseLayout::MultiDiscAlbum,
        _ => ReleaseLayout::StandardAlbum,
    };
    release
}

fn audio_codec(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "flac" => "FLAC",
        "mp3" => "MP3",
        "m4a" => "M4A",
        "aac" => "AAC",
        "opus" => "Opus",
        "ogg" | "oga" => "Ogg",
        "wav" => "WAV",
        "aif" | "aiff" => "AIFF",
        "alac" => "ALAC",
        "ape" => "APE",
        "wv" => "WavPack",
        "wma" => "WMA",
        _ => return None,
    })
}

static DISC_FOLDER: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)^(?:disc|disk|cd)[ _-]*([0-9]{1,2})$").unwrap());
static DISC_TRACK_PREFIX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^([0-9]{1,2})[-.]([0-9]{2,3})[ ._-]+(.+)$").unwrap());
static TRACK_PREFIX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^([0-9]{1,3})[ ._-]+(.+)$").unwrap());

pub(super) fn track_title<'a>(folders: &[String], stem: &'a str) -> (Option<u32>, Option<u32>, &'a str) {
    let stem = stem.trim();
    let disc = folders.iter().rev().find_map(|folder| {
        let captures = DISC_FOLDER.captures(folder.trim())?;
        positive_number(captures.get(1)?.as_str())
    });
    if let Some(captures) = DISC_TRACK_PREFIX.captures(stem) {
        let prefix_disc = captures
            .get(1)
            .and_then(|number| positive_number(number.as_str()));
        let track = captures
            .get(2)
            .and_then(|number| positive_number(number.as_str()));
        if let (Some(prefix_disc), Some(track), Some(title)) = (prefix_disc, track, captures.get(3))
        {
            if disc.is_none_or(|disc| disc == prefix_disc) {
                return (Some(prefix_disc), Some(track), title.as_str());
            }
            return (None, None, stem);
        }
    }
    if let Some(captures) = TRACK_PREFIX.captures(stem) {
        let track = captures
            .get(1)
            .and_then(|number| positive_number(number.as_str()));
        if let (Some(track), Some(title)) = (track, captures.get(2)) {
            return (disc, Some(track), title.as_str());
        }
    }
    (disc, None, stem)
}

fn positive_number(text: &str) -> Option<u32> {
    text.parse().ok().filter(|number| *number > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_sidecars_are_not_audio_tracks() {
        let files = [("01 Romantic Theory.mp3", 7_000_000), ("._01 Romantic Theory.mp3", 4096)]
            .into_iter()
            .enumerate()
            .map(|(index, (name, length))| RawFile {
                index,
                components: vec![name.into()],
                path: name.into(),
                length,
                padding: false,
            })
            .collect();
        let release = analyze_release(String::new(), "Monomania".into(), 7_004_096, files);
        assert_eq!(release.tracks.len(), 1);
        assert_eq!(release.tracks[0].title, "Romantic Theory");
    }
}
