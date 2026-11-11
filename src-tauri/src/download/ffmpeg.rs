//! ffmpeg and ffprobe for yt-dlp: `--extract-audio` needs both to turn a download that is not already a plain
//! audio file (YouTube's Opus in WebM) into one. An ffmpeg the user installed is used when there is one; otherwise
//! the app keeps its own static build (eugeneware/ffmpeg-static), on Linux, Windows and macOS alike.

use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{path::BaseDirectory, AppHandle, Manager};

const RELEASE_API: &str = "https://api.github.com/repos/eugeneware/ffmpeg-static/releases/latest";
/// Every binary is downloaded from this release folder and nowhere else.
const DOWNLOAD_PREFIX: &str = "https://github.com/eugeneware/ffmpeg-static/releases/download/";
const TOOLS: [&str; 2] = ["ffmpeg", "ffprobe"];

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    /// "sha256:<hex>", computed by GitHub when the asset was uploaded.
    digest: Option<String>,
}

/// The file name of `tool` on `os`. Pure.
fn exe_name(tool: &str, os: &str) -> String {
    if os == "windows" {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    }
}

/// ffmpeg-static's name for this OS and CPU (`std::env::consts`), if it builds for it. Windows on ARM runs the x64
/// build. Pure.
fn platform_suffix(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64" | "aarch64") => "win32-x64",
        ("macos", "x86_64") => "darwin-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "arm") => "linux-arm",
        ("linux", "x86") => "linux-ia32",
        _ => return None,
    })
}

/// The release asset holding `tool` for a platform: the gzipped binary. Pure.
fn asset_name(tool: &str, suffix: &str) -> String {
    format!("{tool}-{suffix}.gz")
}

/// The lowercase hex of a GitHub asset digest ("sha256:<64 hex>"), `None` for anything else. Pure.
fn sha256_of_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_ascii_lowercase())
}

/// The first folder in `dirs` that holds both ffmpeg and ffprobe. Pure apart from `exists`.
fn dir_with_both(dirs: impl IntoIterator<Item = PathBuf>, os: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dirs.into_iter().find(|dir| TOOLS.iter().all(|tool| exists(&dir.join(exe_name(tool, os)))))
}

/// An ffmpeg the user installed: on PATH, or where Homebrew and distributions put it (an app started from the macOS
/// Dock does not get the shell's PATH, so /opt/homebrew/bin is missing from it).
fn system_dir() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(unix) {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    dir_with_both(dirs, std::env::consts::OS, Path::is_file)
}

/// Where the app keeps its own ffmpeg and ffprobe.
fn app_dir(app: &AppHandle) -> PathBuf {
    app.path().resolve("ffmpeg", BaseDirectory::AppLocalData).unwrap_or_default()
}

/// yt-dlp's arguments to find ffmpeg: the user's own, else the app's, else none (yt-dlp then reports it missing).
pub fn location_args(app: &AppHandle) -> Vec<String> {
    let own = app_dir(app);
    system_dir()
        .or_else(|| dir_with_both([own], std::env::consts::OS, Path::is_file))
        .map(|dir| vec!["--ffmpeg-location".to_string(), dir.to_string_lossy().into_owned()])
        .unwrap_or_default()
}

/// Unpacks one gzipped binary.
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    GzDecoder::new(bytes).read_to_end(&mut out).map_err(|e| format!("cannot unpack ffmpeg: {e}"))?;
    Ok(out)
}

/// Installs or updates the app's ffmpeg and ffprobe, verified against GitHub's checksums. Nothing is downloaded
/// when the user has their own ffmpeg.
pub async fn check_and_update(app: AppHandle) -> Result<(), String> {
    if let Some(dir) = system_dir() {
        println!("[ffmpeg] Using the installed ffmpeg in {dir:?}");
        return Ok(());
    }
    let os = std::env::consts::OS;
    let Some(suffix) = platform_suffix(os, std::env::consts::ARCH) else {
        return Ok(()); // no build for this machine: downloads that need converting report ffmpeg missing
    };
    let client = reqwest::Client::builder()
        .user_agent("blacktape-desktop")
        .build()
        .map_err(|e| e.to_string())?;
    let release: GithubRelease = client
        .get(RELEASE_API)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let dir = app_dir(&app);
    let version_file = dir.join("version");
    let marker = format!("{} {suffix}", release.tag_name.trim());
    let installed = TOOLS.iter().all(|tool| dir.join(exe_name(tool, os)).is_file());
    if installed && fs::read_to_string(&version_file).unwrap_or_default().trim() == marker {
        return Ok(());
    }
    println!("[ffmpeg] Installing {}...", release.tag_name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    for tool in TOOLS {
        let name = asset_name(tool, suffix);
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| format!("{name} not found in the ffmpeg release"))?;
        if !asset.browser_download_url.starts_with(DOWNLOAD_PREFIX) {
            return Err(format!("unexpected ffmpeg download url {}", asset.browser_download_url));
        }
        let expected = asset
            .digest
            .as_deref()
            .and_then(sha256_of_digest)
            .ok_or_else(|| format!("no checksum for {name}"))?;
        let gz = client
            .get(&asset.browser_download_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let actual: String = Sha256::digest(&gz).iter().map(|b| format!("{b:02x}")).collect();
        if actual != expected {
            return Err(format!("{name} checksum mismatch: expected {expected}, got {actual}"));
        }
        let binary = gunzip(&gz)?;

        // written beside the binary and swapped in whole, so a failed install never leaves half a file
        let target = dir.join(exe_name(tool, os));
        let partial = target.with_extension("download");
        let written = fs::write(&partial, &binary).map_err(|e| e.to_string()).and_then(|()| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&partial, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
            fs::rename(&partial, &target).map_err(|e| format!("cannot replace {tool}: {e}"))
        });
        if let Err(e) = written {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
    }
    fs::write(&version_file, &marker).map_err(|e| e.to_string())?;
    println!("[ffmpeg] Install complete!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Write;

    fn file_set(paths: &[&str]) -> HashSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn windows_binaries_end_in_exe_and_other_systems_do_not() {
        assert_eq!(exe_name("ffmpeg", "windows"), "ffmpeg.exe");
        assert_eq!(exe_name("ffprobe", "windows"), "ffprobe.exe");
        assert_eq!(exe_name("ffmpeg", "linux"), "ffmpeg");
        assert_eq!(exe_name("ffmpeg", "macos"), "ffmpeg");
    }

    #[test]
    fn windows_on_x64_and_arm_uses_the_x64_build() {
        assert_eq!(platform_suffix("windows", "x86_64"), Some("win32-x64"));
        assert_eq!(platform_suffix("windows", "aarch64"), Some("win32-x64"));
    }

    #[test]
    fn macos_builds_are_split_by_cpu() {
        assert_eq!(platform_suffix("macos", "x86_64"), Some("darwin-x64"));
        assert_eq!(platform_suffix("macos", "aarch64"), Some("darwin-arm64"));
    }

    #[test]
    fn linux_builds_cover_each_cpu() {
        assert_eq!(platform_suffix("linux", "x86_64"), Some("linux-x64"));
        assert_eq!(platform_suffix("linux", "aarch64"), Some("linux-arm64"));
        assert_eq!(platform_suffix("linux", "arm"), Some("linux-arm"));
        assert_eq!(platform_suffix("linux", "x86"), Some("linux-ia32"));
    }

    #[test]
    fn unknown_platforms_have_no_build() {
        assert_eq!(platform_suffix("freebsd", "x86_64"), None);
        assert_eq!(platform_suffix("macos", "x86"), None);
    }

    #[test]
    fn asset_is_the_gzipped_binary_for_the_platform() {
        assert_eq!(asset_name("ffprobe", "linux-x64"), "ffprobe-linux-x64.gz");
    }

    #[test]
    fn sha256_digest_hex_is_returned_lowercased() {
        let digest = format!("sha256:{}", "AB".repeat(32));
        assert_eq!(sha256_of_digest(&digest), Some("ab".repeat(32)));
    }

    #[test]
    fn digest_without_a_sha256_prefix_is_refused() {
        assert_eq!(sha256_of_digest(&"a".repeat(64)), None);
        assert_eq!(sha256_of_digest(&format!("sha512:{}", "a".repeat(128))), None);
    }

    #[test]
    fn sha256_digest_must_have_exactly_64_hex_digits() {
        assert_eq!(sha256_of_digest(&format!("sha256:{}", "a".repeat(63))), None);
        assert_eq!(sha256_of_digest(&format!("sha256:{}g", "a".repeat(63))), None);
    }

    #[test]
    fn first_folder_holding_both_tools_is_chosen() {
        let files = file_set(&["/a/ffmpeg", "/b/ffmpeg", "/b/ffprobe", "/c/ffmpeg", "/c/ffprobe"]);
        let dirs = ["/a", "/b", "/c"].map(PathBuf::from);
        assert_eq!(dir_with_both(dirs, "linux", |p| files.contains(p)), Some(PathBuf::from("/b")));
    }

    #[test]
    fn no_folder_is_chosen_when_none_holds_both_tools() {
        let files = file_set(&["/a/ffmpeg", "/b/ffprobe"]);
        let dirs = ["/a", "/b"].map(PathBuf::from);
        assert_eq!(dir_with_both(dirs, "linux", |p| files.contains(p)), None);
    }

    #[test]
    fn windows_folders_need_the_exe_names() {
        let files = file_set(&["/a/ffmpeg", "/a/ffprobe", "/b/ffmpeg.exe", "/b/ffprobe.exe"]);
        let dirs = ["/a", "/b"].map(PathBuf::from);
        assert_eq!(dir_with_both(dirs.clone(), "windows", |p| files.contains(p)), Some(PathBuf::from("/b")));
        assert_eq!(dir_with_both(["/a"].map(PathBuf::from), "windows", |p| files.contains(p)), None);
    }

    #[test]
    fn gunzip_returns_the_bytes_that_were_compressed() {
        let data = b"ffprobe binary".to_vec();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&data).unwrap();
        let gz = encoder.finish().unwrap();
        assert_eq!(gunzip(&gz).unwrap(), data);
    }

    #[test]
    fn gunzip_refuses_data_that_is_not_gzip() {
        assert!(gunzip(b"not gzip").is_err());
    }
}
