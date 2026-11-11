//! The cookies yt-dlp gets: the session of the in-app login window (`auth::youtube::profile_dir`). Its browser
//! engine, and so the way it stores cookies, differs per platform.

use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieSource {
    /// No session yt-dlp can read (not signed in, or a platform whose engine keeps cookies elsewhere): download
    /// without cookies, which public tracks do not need.
    None,
    /// Windows: WebView2 keeps a Chromium profile that yt-dlp reads as Edge (`edge:<profile dir>`).
    Browser(String),
    /// Linux: WebKitGTK keeps the cookies in a Netscape cookies.txt file.
    File(PathBuf),
}

/// Where the login window keeps its cookies, if anywhere. yt-dlp fails outright on a browser profile without a
/// cookie database, so a missing one means no cookies rather than a broken argument.
pub fn find(profile_dir: &Path) -> CookieSource {
    let edge = profile_dir.join("EBWebView").join("Default");
    if edge.join("Network").join("Cookies").is_file() || edge.join("Cookies").is_file() {
        return CookieSource::Browser(format!("edge:{}", edge.to_string_lossy()));
    }
    // wry gives WebKitGTK `<data directory>/cookies` as its cookie file
    let text = profile_dir.join("cookies");
    if text.is_file() {
        return CookieSource::File(text);
    }
    CookieSource::None
}

/// A private copy of the cookie file for one yt-dlp run, deleted when dropped. yt-dlp writes its cookie jar back
/// into the file it is given, so it never gets the login window's own file, and parallel runs never share one.
#[derive(Debug)]
pub struct CookieCopy(PathBuf);

impl Drop for CookieCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The header yt-dlp requires on a cookies.txt; WebKitGTK writes the lines without it.
const NETSCAPE_HEADER: &str = "# Netscape HTTP Cookie File\n";

/// The cookie file's content as yt-dlp reads it: the Netscape header first, and each cookie cut to the 7 Netscape
/// fields. libsoup (WebKitGTK) appends an 8th, SameSite, and yt-dlp skips every line that has it ("invalid length
/// 8"), so without this it got no cookies at all. Pure.
fn to_netscape(content: &str) -> String {
    let mut out = String::with_capacity(content.len() + NETSCAPE_HEADER.len());
    if !content.starts_with("# Netscape HTTP Cookie File") && !content.starts_with("# HTTP Cookie File") {
        out.push_str(NETSCAPE_HEADER);
    }
    for line in content.lines() {
        let is_cookie = !line.trim().is_empty() && (!line.starts_with('#') || line.starts_with("#HttpOnly_"));
        if is_cookie {
            let fields: Vec<&str> = line.split('\t').collect();
            out.push_str(&fields[..fields.len().min(7)].join("\t"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

impl CookieSource {
    /// yt-dlp's cookie arguments for one run. A file source is first copied to `copy_path` (readable by this user
    /// only); keep the returned guard alive until yt-dlp exits, it deletes the copy.
    pub fn args(&self, copy_path: &Path) -> std::io::Result<(Vec<String>, Option<CookieCopy>)> {
        match self {
            CookieSource::None => Ok((Vec::new(), None)),
            CookieSource::Browser(spec) => Ok((vec!["--cookies-from-browser".into(), spec.clone()], None)),
            CookieSource::File(file) => {
                let content = std::fs::read_to_string(file)?;
                if let Some(parent) = copy_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let guard = CookieCopy(copy_path.to_path_buf());
                options.open(copy_path)?.write_all(to_netscape(&content).as_bytes())?;
                Ok((vec!["--cookies".into(), copy_path.to_string_lossy().into_owned()], Some(guard)))
            }
        }
    }
}

/// Folder for the per-run copies, inside the app's own data folder (never the music library, which may be synced).
pub fn copies_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("cookie-copies")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tabc\n";

    #[test]
    fn no_profile_means_no_cookies() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find(dir.path()), CookieSource::None);
        assert_eq!(find(&dir.path().join("missing")), CookieSource::None);
    }

    #[test]
    fn an_edge_profile_without_a_cookie_database_is_not_used() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("EBWebView").join("Default")).unwrap();
        assert_eq!(find(dir.path()), CookieSource::None);
    }

    #[test]
    fn webview2_cookies_are_read_from_the_edge_profile() {
        let dir = tempfile::tempdir().unwrap();
        let network = dir.path().join("EBWebView").join("Default").join("Network");
        std::fs::create_dir_all(&network).unwrap();
        std::fs::write(network.join("Cookies"), b"").unwrap();
        let profile = dir.path().join("EBWebView").join("Default");
        assert_eq!(find(dir.path()), CookieSource::Browser(format!("edge:{}", profile.to_string_lossy())));
    }

    #[test]
    fn webkitgtk_cookies_are_read_from_the_text_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cookies"), LINE).unwrap();
        assert_eq!(find(dir.path()), CookieSource::File(dir.path().join("cookies")));
    }

    #[test]
    fn no_cookies_give_no_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let (args, copy) = CookieSource::None.args(&dir.path().join("c.txt")).unwrap();
        assert!(args.is_empty());
        assert!(copy.is_none());
    }

    #[test]
    fn browser_cookies_are_passed_as_a_browser_profile() {
        let dir = tempfile::tempdir().unwrap();
        let (args, copy) = CookieSource::Browser("edge:C:/p".into()).args(&dir.path().join("c.txt")).unwrap();
        assert_eq!(args, ["--cookies-from-browser", "edge:C:/p"]);
        assert!(copy.is_none());
    }

    #[test]
    fn a_cookie_file_is_passed_as_a_headed_private_copy_that_is_deleted_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("cookies");
        std::fs::write(&source, LINE).unwrap();
        let copy_path = dir.path().join("copies").join("run.txt");

        let (args, copy) = CookieSource::File(source.clone()).args(&copy_path).unwrap();
        assert_eq!(args, ["--cookies".to_string(), copy_path.to_string_lossy().into_owned()]);
        assert_eq!(std::fs::read_to_string(&copy_path).unwrap(), format!("{NETSCAPE_HEADER}{LINE}"));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), LINE, "the login window's file is untouched");

        drop(copy);
        assert!(!copy_path.exists());
    }

    #[test]
    fn a_file_that_already_has_the_header_is_copied_as_is() {
        let headed = format!("{NETSCAPE_HEADER}{LINE}");
        assert_eq!(to_netscape(&headed), headed);
        assert_eq!(to_netscape("# HTTP Cookie File\n"), "# HTTP Cookie File\n");
    }

    #[test]
    fn the_samesite_field_webkitgtk_appends_is_dropped() {
        let webkit = "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tabc\tLax\n\
                      .google.com\tTRUE\t/\tFALSE\t1893456000\tSID\txyz\tNone\n";
        let expected = format!(
            "{NETSCAPE_HEADER}#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tLOGIN_INFO\tabc\n\
             .google.com\tTRUE\t/\tFALSE\t1893456000\tSID\txyz\n"
        );
        assert_eq!(to_netscape(webkit), expected);
    }

    #[test]
    fn comments_and_blank_lines_are_kept_unchanged() {
        let content = "# a\tcomment\twith\ttabs\ta\tb\tc\td\te\n\n";
        assert_eq!(to_netscape(content), format!("{NETSCAPE_HEADER}{content}"));
    }
}
