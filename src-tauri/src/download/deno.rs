//! The JavaScript runtime yt-dlp needs for YouTube: without one it cannot solve YouTube's signature and "n"
//! challenges, and YouTube then answers with images only ("Requested format is not available") or "The page
//! needs to be reloaded". A deno the user installed is used when it is new enough; otherwise the app keeps its
//! own next to yt-dlp, so YouTube works with nothing installed.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{path::BaseDirectory, AppHandle, Manager};

/// yt-dlp's challenge solver needs deno 2.0 or newer.
const MIN_MAJOR: u32 = 2;

/// The version in `deno --version` output ("deno 2.7.7 (stable, release, x86_64-pc-windows-msvc)"). Pure.
fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let version = output.lines().next()?.trim().strip_prefix("deno ")?.split_whitespace().next()?;
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// The first candidate that exists and whose version yt-dlp can use. Pure apart from `exists` and `version_of`.
fn first_usable(
    candidates: impl IntoIterator<Item = PathBuf>,
    exists: impl Fn(&Path) -> bool,
    version_of: impl Fn(&Path) -> Option<(u32, u32, u32)>,
) -> Option<PathBuf> {
    candidates
        .into_iter()
        .filter(|path| exists(path))
        .find(|path| version_of(path).is_some_and(|(major, _, _)| major >= MIN_MAJOR))
}

/// Where a deno the user installed can be: on PATH, in deno's own install folder, or where distributions and
/// Homebrew put it.
fn system_candidates(os: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os(if os == "windows" { "USERPROFILE" } else { "HOME" }) {
        dirs.push(PathBuf::from(home).join(".deno").join("bin"));
    }
    if os != "windows" {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    }
    // a relative entry (".") would run whatever deno sits in the current folder
    dirs.into_iter().filter(|dir| dir.is_absolute()).map(|dir| dir.join(binary_name(os))).collect()
}

/// `deno --version` of the binary at `path`.
fn version_of(path: &Path) -> Option<(u32, u32, u32)> {
    let mut cmd = std::process::Command::new(path);
    cmd.arg("--version").stdin(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok().filter(|o| o.status.success())?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// A deno the user installed that yt-dlp can use. Looked up once per app run: it starts a process.
fn system_deno() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| first_usable(system_candidates(std::env::consts::OS), Path::is_file, version_of))
        .as_deref()
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

const RELEASES: &str = "https://github.com/denoland/deno/releases";

/// Where the app keeps deno.
pub fn get_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().resolve("", BaseDirectory::AppLocalData).unwrap_or_default();
    dir.join(binary_name(std::env::consts::OS))
}

fn binary_name(os: &str) -> &'static str {
    if os == "windows" {
        "deno.exe"
    } else {
        "deno"
    }
}

/// deno's release zip for this OS and CPU (`std::env::consts`), if deno builds one. Pure.
fn release_asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "deno-x86_64-pc-windows-msvc.zip",
        ("windows", "aarch64") => "deno-aarch64-pc-windows-msvc.zip",
        ("macos", "x86_64") => "deno-x86_64-apple-darwin.zip",
        ("macos", "aarch64") => "deno-aarch64-apple-darwin.zip",
        ("linux", "x86_64") => "deno-x86_64-unknown-linux-gnu.zip",
        ("linux", "aarch64") => "deno-aarch64-unknown-linux-gnu.zip",
        _ => return None,
    })
}

/// The SHA-256 in one of deno's `<asset>.sha256sum` files, lowercase. Linux and macOS publish sha256sum output
/// ("<hex>  <name>"), Windows publishes PowerShell's Get-FileHash ("Hash : <HEX>"); both hold exactly one 64-digit
/// hex word. Pure.
fn sha256_in(sums: &str) -> Option<String> {
    let mut hashes = sums
        .split(|c: char| c.is_whitespace() || c == ':')
        .filter(|w| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit()));
    let hash = hashes.next()?;
    hashes.next().is_none().then(|| hash.to_ascii_lowercase())
}

/// yt-dlp's arguments to use the user's deno, else the app's, else none (yt-dlp then looks for one itself).
pub fn runtime_args(app: &AppHandle) -> Vec<String> {
    match system_deno() {
        Some(deno) => runtime_args_for(deno),
        None => runtime_args_for(&get_path(app)),
    }
}

fn runtime_args_for(deno: &Path) -> Vec<String> {
    if deno.is_file() {
        vec!["--js-runtimes".to_string(), format!("deno:{}", deno.to_string_lossy())]
    } else {
        Vec::new()
    }
}

/// The deno binary inside a release zip. Only the entry named exactly `name` is read, nothing is extracted by
/// the paths the archive gives.
fn binary_from_zip(zip: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
    let mut entry = archive.by_name(name).map_err(|e| format!("{name} not found in the deno zip: {e}"))?;
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, String> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())
}

/// Installs or updates the app's deno to the latest release, verified against its published checksum. Nothing is
/// downloaded when the user has a usable deno, and an earlier app copy is then removed (about 100 MB).
pub async fn check_and_update(app: AppHandle) -> Result<(), String> {
    // the lookup starts `deno --version`: keep it off the async workers
    let system = tauri::async_runtime::spawn_blocking(|| system_deno().map(Path::to_path_buf))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(deno) = system {
        println!("[deno] Using the installed deno at {deno:?}");
        let own = get_path(&app);
        if own.is_file() {
            // Windows refuses while the app's copy runs; it is then removed on a later start
            if fs::remove_file(&own).is_ok() {
                let _ = fs::remove_file(own.with_extension("version"));
            }
        }
        return Ok(());
    }
    let os = std::env::consts::OS;
    let Some(asset) = release_asset(os, std::env::consts::ARCH) else {
        return Ok(()); // no deno build for this machine: yt-dlp falls back to a runtime it finds itself
    };
    let client = reqwest::Client::builder()
        .user_agent("blacktape-desktop")
        .build()
        .map_err(|e| e.to_string())?;

    let release: GithubRelease = fetch(&client, "https://api.github.com/repos/denoland/deno/releases/latest")
        .await?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let tag = release.tag_name.trim();
    let binary_path = get_path(&app);
    let version_file = binary_path.with_extension("version");
    let marker = format!("{tag} {asset}");
    if binary_path.is_file() && fs::read_to_string(&version_file).unwrap_or_default().trim() == marker {
        return Ok(());
    }
    println!("[deno] Installing {tag}...");

    // the tag goes into a url path: only a plain version ("v2.9.8") is accepted
    if !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err(format!("unexpected deno release tag {tag:?}"));
    }
    let base = format!("{RELEASES}/download/{tag}/{asset}");
    let sums = fetch(&client, &format!("{base}.sha256sum")).await?.text().await.map_err(|e| e.to_string())?;
    let expected = sha256_in(&sums).ok_or_else(|| format!("no checksum in {asset}.sha256sum"))?;
    let zip = fetch(&client, &base).await?.bytes().await.map_err(|e| e.to_string())?;
    let actual: String = Sha256::digest(&zip).iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        return Err(format!("deno checksum mismatch: expected {expected}, got {actual}"));
    }
    let binary = binary_from_zip(&zip, binary_name(os))?;

    if let Some(parent) = binary_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // written beside the binary and swapped in whole, so a failed install never leaves half a deno
    let partial_path = binary_path.with_extension("download");
    let written = fs::write(&partial_path, &binary).map_err(|e| e.to_string()).and_then(|()| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&partial_path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        fs::rename(&partial_path, &binary_path).map_err(|e| format!("cannot replace deno: {e}"))
    });
    if let Err(e) = written {
        let _ = fs::remove_file(&partial_path);
        return Err(e);
    }
    fs::write(&version_file, &marker).map_err(|e| e.to_string())?;
    println!("[deno] Install complete!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const HASH: &str = "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609";

    #[test]
    fn every_desktop_platform_has_a_release_asset() {
        assert_eq!(release_asset("linux", "x86_64"), Some("deno-x86_64-unknown-linux-gnu.zip"));
        assert_eq!(release_asset("linux", "aarch64"), Some("deno-aarch64-unknown-linux-gnu.zip"));
        assert_eq!(release_asset("windows", "x86_64"), Some("deno-x86_64-pc-windows-msvc.zip"));
        assert_eq!(release_asset("macos", "aarch64"), Some("deno-aarch64-apple-darwin.zip"));
        assert_eq!(release_asset("linux", "arm"), None);
    }

    #[test]
    fn version_is_read_from_deno_version_output() {
        let out = "deno 2.7.7 (stable, release, x86_64-pc-windows-msvc)\nv8 14.6.202.9-rusty\ntypescript 5.9.2\n";
        assert_eq!(parse_version(out), Some((2, 7, 7)));
        assert_eq!(parse_version("deno 1.46.3 (release, x86_64-unknown-linux-gnu)"), Some((1, 46, 3)));
    }

    #[test]
    fn output_that_is_not_deno_version_has_no_version() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("node v22.1.0"), None);
        assert_eq!(parse_version("deno 2.x.1"), None);
        assert_eq!(parse_version("deno 2.7"), None);
    }

    #[test]
    fn the_first_installed_deno_2_or_newer_is_chosen() {
        let candidates = ["/a/deno", "/b/deno", "/c/deno", "/d/deno"].map(PathBuf::from);
        let exists = |p: &Path| p != Path::new("/a/deno");
        let version = |p: &Path| match p.to_str() {
            Some("/b/deno") => Some((1, 46, 3)), // too old for yt-dlp
            Some("/c/deno") => Some((2, 0, 0)),
            _ => Some((2, 9, 8)),
        };
        assert_eq!(first_usable(candidates, exists, version), Some(PathBuf::from("/c/deno")));
    }

    #[test]
    fn no_deno_is_chosen_when_none_is_usable() {
        let candidates = ["/a/deno", "/b/deno"].map(PathBuf::from);
        let version = |p: &Path| (p == Path::new("/a/deno")).then_some((1, 0, 0));
        assert_eq!(first_usable(candidates, |_| true, version), None);
    }

    #[test]
    fn system_candidates_are_absolute_and_named_for_the_platform() {
        let os = std::env::consts::OS;
        let candidates = system_candidates(os);
        assert!(candidates.iter().all(|p| p.is_absolute()), "{candidates:?}");
        assert!(candidates.iter().all(|p| p.file_name() == Some(binary_name(os).as_ref())));
        #[cfg(unix)]
        assert!(candidates.contains(&PathBuf::from("/usr/bin/deno")));
    }

    #[test]
    fn checksum_is_read_from_sha256sum_output() {
        let sums = format!("{HASH}  deno-x86_64-unknown-linux-gnu.zip\n");
        assert_eq!(sha256_in(&sums), Some(HASH.to_string()));
    }

    #[test]
    fn checksum_is_read_from_powershell_output_and_lowercased() {
        let sums = format!(
            "\r\nAlgorithm : SHA256\r\nHash      : {}\r\nPath      : C:\\a\\deno.zip\r\n\r\n",
            HASH.to_ascii_uppercase()
        );
        assert_eq!(sha256_in(&sums), Some(HASH.to_string()));
    }

    #[test]
    fn a_checksum_file_without_exactly_one_hash_is_refused() {
        assert_eq!(sha256_in(""), None);
        assert_eq!(sha256_in(&format!("{}  deno.zip", &HASH[1..])), None);
        assert_eq!(sha256_in(&format!("{HASH}  a.zip\n{HASH}  b.zip\n")), None);
    }

    #[test]
    fn yt_dlp_gets_the_runtime_only_once_it_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let deno = dir.path().join("deno");
        assert!(runtime_args_for(&deno).is_empty());
        fs::write(&deno, b"").unwrap();
        assert_eq!(runtime_args_for(&deno), ["--js-runtimes".to_string(), format!("deno:{}", deno.display())]);
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn only_the_deno_entry_is_taken_from_the_zip() {
        let zip = zip_with(&[("../evil", b"no"), ("deno", b"binary")]);
        assert_eq!(binary_from_zip(&zip, "deno").unwrap(), b"binary");
        assert!(binary_from_zip(&zip_with(&[("other", b"x")]), "deno").is_err());
        assert!(binary_from_zip(b"not a zip", "deno").is_err());
    }
}
