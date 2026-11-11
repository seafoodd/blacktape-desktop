use crate::types::QualityTier;
use lofty::file::AudioFile;
use std::path::Path;
use winapi::um::fileapi;
use winapi::um::winnt::FILE_ATTRIBUTE_HIDDEN;

pub fn set_hidden(path: &Path, hidden: bool) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::prelude::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let current = fileapi::GetFileAttributesW(wide.as_ptr());
            if current != fileapi::INVALID_FILE_ATTRIBUTES {
                let new_attrs = if hidden {
                    current | FILE_ATTRIBUTE_HIDDEN
                } else {
                    current & !FILE_ATTRIBUTE_HIDDEN
                };
                if fileapi::SetFileAttributesW(wide.as_ptr(), new_attrs) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
            } else {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

pub fn force_remove_dir_all(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if std::fs::remove_dir_all(path).is_ok() {
        return Ok(());
    }

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let _ = force_remove_dir_all(&p);
            } else {
                if let Ok(metadata) = std::fs::metadata(&p) {
                    let mut perms = metadata.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    let _ = std::fs::set_permissions(&p, perms);
                }
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    std::fs::remove_dir_all(path)
}

pub fn sanitize(name: &str) -> String {
    let unescaped = html_escape::decode_html_entities(name);
    let sanitized: String =
        unescaped.replace(['/', '\\', '?', '%', '*', ':', '|', '"', '<', '>'], "");
    if sanitized.starts_with('.') {
        return sanitized[1..].to_string();
    }
    sanitized
}

pub fn sanitize_fs(name: &str) -> String {
    let unescaped = html_escape::decode_html_entities(name);
    let replaced = unescaped.replace(
        ['/', '\\', '?', '%', '*', ':', '|', '"', '<', '>', '.'],
        " ",
    );
    let sanitized: String = replaced.split_whitespace().collect::<Vec<_>>().join(" ");

    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

pub fn remove_empty_parents_up_to(path: &Path, root_dir: &Path) {
    let mut current = path;

    while let Some(parent) = current.parent() {
        if parent == root_dir || !parent.starts_with(root_dir) {
            break;
        }

        match std::fs::read_dir(parent) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    println!("[Cleanup] Removing empty parent directory: {:?}", parent);
                    if let Err(e) = std::fs::remove_dir(parent) {
                        eprintln!(
                            "[Cleanup Error] Failed to remove empty parent {:?}: {}",
                            parent, e
                        );
                        break;
                    }
                } else {
                    break;
                }
            }
            Err(_) => break,
        }

        current = parent;
    }
}

pub fn make_canonical_slug(artist: &str, entity: &str) -> String {
    let combined = format!("{} {}", artist, entity);
    combined
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

/// Whether an MP4 audio stream (.m4a) is lossless (ALAC) rather than AAC. Only lossless codecs report a bit depth;
/// a bitrate no AAC encoder reaches (500 kbps and more) is the fallback when the depth is missing. Pure.
pub fn is_lossless_mp4(bit_depth: Option<u8>, bitrate_kbps: u32) -> bool {
    bit_depth.is_some() || bitrate_kbps >= 500
}

pub fn determine_quality(ext: &str, tagged_file: &lofty::file::TaggedFile) -> QualityTier {
    let bitrate = tagged_file.properties().audio_bitrate().unwrap_or(0);

    match ext.to_lowercase().as_str() {
        "flac" | "wav" | "alac" | "aiff" => QualityTier::Lossless,

        "opus" => {
            if bitrate >= 128 {
                QualityTier::High
            } else if bitrate >= 64 {
                QualityTier::Standard
            } else {
                QualityTier::Low
            }
        }

        // ALAC also lives in .m4a: it is lossless, not an AAC bitrate tier
        "m4a" | "mp4" if is_lossless_mp4(tagged_file.properties().bit_depth(), bitrate) => QualityTier::Lossless,

        "m4a" | "aac" | "ogg" => {
            if bitrate >= 192 {
                QualityTier::High
            } else if bitrate >= 128 {
                QualityTier::Standard
            } else {
                QualityTier::Low
            }
        }

        "mp3" => {
            if bitrate >= 256 {
                QualityTier::High
            } else if bitrate >= 192 {
                QualityTier::Standard
            } else {
                QualityTier::Low
            }
        }

        _ => QualityTier::Low,
    }
}

#[cfg(test)]
mod tests {
    use crate::utils::{is_lossless_mp4, sanitize, sanitize_fs};

    #[test]
    fn test_sanitize_removes_illegal_windows_chars() {
        let dirty_name = "Is This Love? (Feat. Artist) *Remix* / <Live>";
        let expected = "Is This Love (Feat. Artist) Remix  Live";

        assert_eq!(sanitize(dirty_name), expected);
    }

    #[test]
    fn test_sanitize_fs_comprehensive() {
        // Basic case with mixed illegal characters and periods
        assert_eq!(
            sanitize_fs("Is This Love? (Feat. Artist) *Remix* / <Live>"),
            "Is This Love (Feat Artist) Remix Live"
        );

        // Leading/trailing whitespace and illegal start characters
        assert_eq!(sanitize_fs("  .Song/Name  "), "Song Name");

        // Multiple consecutive illegal characters
        assert_eq!(sanitize_fs("Artist:::Title|||Remix"), "Artist Title Remix");

        // Strings that would result in empty/whitespace-only (fallback test)
        // If your implementation returns "unknown_track" for empty strings:
        assert_eq!(sanitize_fs("///..."), "unknown");

        // HTML entities (ensuring decode happens before replace)
        assert_eq!(sanitize_fs("Artist &amp; Title"), "Artist & Title");

        // Ensure standard characters are preserved
        assert_eq!(sanitize_fs("Normal-Name_123"), "Normal-Name_123");
    }

    #[test]
    fn test_sanitize_handles_empty_or_normal_strings() {
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("NormalSong123"), "NormalSong123");
    }

    #[test]
    fn is_lossless_mp4_true_for_alac_with_bit_depth() {
        assert!(is_lossless_mp4(Some(16), 935));
    }

    #[test]
    fn is_lossless_mp4_true_for_bit_depth_even_at_zero_bitrate() {
        assert!(is_lossless_mp4(Some(24), 0));
    }

    #[test]
    fn is_lossless_mp4_false_for_aac_bitrates_without_depth() {
        assert!(!is_lossless_mp4(None, 128));
        assert!(!is_lossless_mp4(None, 256));
        assert!(!is_lossless_mp4(None, 320));
    }

    #[test]
    fn is_lossless_mp4_true_at_exact_bitrate_threshold() {
        assert!(is_lossless_mp4(None, 500));
    }

    #[test]
    fn is_lossless_mp4_false_just_below_bitrate_threshold() {
        assert!(!is_lossless_mp4(None, 499));
    }
}

