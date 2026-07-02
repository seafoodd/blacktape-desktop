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
    use crate::utils::sanitize;

    #[test]
    fn test_sanitize_removes_illegal_windows_chars() {
        let dirty_name = "Is This Love? (Feat. Artist) *Remix* / <Live>";
        let expected = "Is This Love (Feat Artist) Remix  Live";

        assert_eq!(sanitize(dirty_name), expected);
    }

    #[test]
    fn test_sanitize_handles_empty_or_normal_strings() {
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("NormalSong123"), "NormalSong123");
    }
}
