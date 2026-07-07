use std::path::Path;
use winapi::um::fileapi;
use winapi::um::winnt::{FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL};

pub fn set_hidden(path: &Path, hidden: bool) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::prelude::OsStrExt;
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            if hidden {
                fileapi::SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN);
            } else {
                fileapi::SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_NORMAL);
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use crate::utils::{sanitize, sanitize_fs};

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
}
