//! Which folders the interface may read files from. It only reads covers (`asset:` urls and `thumb:` thumbnails),
//! so the scope is the app data folder (configured statically), the library folder (recursive, added at start-up and
//! by every scan, since the user can change it) and the folders of the covers already in the database (added at
//! start-up: songs scanned from an earlier library folder keep their covers). The rest of the disk stays closed, so
//! a bug that injects content into the interface cannot read arbitrary files.

use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Folders that contain these cover files, without duplicates; web urls and relative paths are skipped. Pure.
pub fn cover_dirs<'a>(cover_paths: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = cover_paths
        .into_iter()
        .filter(|p| !p.starts_with("http://") && !p.starts_with("https://"))
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Opens a folder to the interface; `recursive` includes its subfolders.
pub fn allow_dir(app: &AppHandle, dir: &Path, recursive: bool) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, recursive) {
        eprintln!("[asset scope] cannot allow {}: {e}", dir.display());
    }
}

/// True when the interface may read this file.
pub fn is_allowed(app: &AppHandle, path: &Path) -> bool {
    app.asset_protocol_scope().is_allowed(path)
}

/// Start-up: the library folder recorded by the last scan, and the folder of every cover in the database (only
/// those folders, not their subfolders).
pub fn allow_known_dirs(app: &AppHandle, library_dir: Option<&str>, cover_paths: &[String]) {
    if let Some(dir) = library_dir.filter(|d| !d.is_empty()) {
        allow_dir(app, Path::new(dir), true);
    }
    for dir in cover_dirs(cover_paths.iter().map(String::as_str)) {
        allow_dir(app, &dir, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    const A: &str = "C:\\music\\Artist\\Album\\cover.jpg";
    #[cfg(windows)]
    const B: &str = "C:\\music\\Artist\\Album\\other.png";
    #[cfg(windows)]
    const C: &str = "D:\\covers\\abc.jpg";
    #[cfg(not(windows))]
    const A: &str = "/music/Artist/Album/cover.jpg";
    #[cfg(not(windows))]
    const B: &str = "/music/Artist/Album/other.png";
    #[cfg(not(windows))]
    const C: &str = "/covers/abc.jpg";

    #[test]
    fn cover_dirs_are_the_parent_folders_without_duplicates() {
        let dirs = cover_dirs([A, B, C]);
        assert_eq!(dirs.len(), 2);
        assert!(dirs.contains(&Path::new(A).parent().unwrap().to_path_buf()));
        assert!(dirs.contains(&Path::new(C).parent().unwrap().to_path_buf()));
    }

    #[test]
    fn cover_dirs_skip_web_urls_relative_and_empty_paths() {
        let dirs = cover_dirs(["https://f4.bcbits.com/img/a.jpg", "http://x/y.jpg", "cover.jpg", "", "Album/cover.jpg"]);
        assert!(dirs.is_empty());
    }
}
