use crate::download::{ProgressEvent, TrackDownload};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::time::sleep;

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn get_path(app: &AppHandle) -> PathBuf {
    let mut path = app
        .path()
        .resolve("", BaseDirectory::AppLocalData)
        .unwrap_or_default();

    #[cfg(target_os = "windows")]
    path.push("yt-dlp.exe");
    #[cfg(not(target_os = "windows"))]
    path.push("yt-dlp");

    path
}

/// The release asset listing the SHA-256 of every yt-dlp binary ("<hex>  <file name>" per line).
const CHECKSUMS_ASSET: &str = "SHA2-256SUMS";

/// The lowercase SHA-256 that `sums` (the SHA2-256SUMS file) lists for `asset`, if it lists one. Pure.
fn expected_sha256(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*'); // "*name" marks binary mode in sha256sum output
        let valid = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
        (name == asset && valid).then(|| hash.to_ascii_lowercase())
    })
}

/// The yt-dlp release asset for this OS and CPU (`std::env::consts`). On Linux the standalone build, not the plain
/// `yt-dlp` zipapp: that one runs on the system Python without curl_cffi, so it cannot impersonate a browser and
/// Bandcamp refuses it ("no impersonate target is available"). Pure.
fn release_asset(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("windows", _) => "yt-dlp.exe",
        ("macos", _) => "yt-dlp_macos",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("linux", "arm") => "yt-dlp_linux_armv7l",
        _ => "yt-dlp",
    }
}

/// What the `.version` file holds for an installed binary: release tag and asset. Pure.
fn installed_marker(tag: &str, asset: &str) -> String {
    format!("{} {asset}", tag.trim())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn check_and_update(app: AppHandle) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("blacktape-desktop")
        .build()
        .map_err(|e| e.to_string())?;

    let release_url = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
    let response = client
        .get(release_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<GithubRelease>()
        .await
        .map_err(|e| e.to_string())?;

    let target_asset = release_asset(std::env::consts::OS, std::env::consts::ARCH);
    // The marker names the asset too, so a switch to another asset (yt-dlp -> yt-dlp_linux) replaces the binary
    // even when the version did not change.
    let latest_version = installed_marker(&response.tag_name, target_asset);
    let binary_path = get_path(&app);
    let version_file = binary_path.with_extension("version");
    let local_version = fs::read_to_string(&version_file).unwrap_or_default();

    if !binary_path.exists() || local_version.trim() != latest_version {
        println!("[ytdlp] Upgrading binary to {}...", latest_version);

        let asset_url = |name: &str| {
            response
                .assets
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.browser_download_url.clone())
                .ok_or_else(|| format!("{name} not found in the yt-dlp release"))
        };
        let binary_url = asset_url(target_asset)?;
        let sums_url = asset_url(CHECKSUMS_ASSET)?;

        // The published checksum comes first: no binary is written without one to compare with.
        let sums = client
            .get(&sums_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let expected = expected_sha256(&sums, target_asset)
            .ok_or_else(|| format!("{CHECKSUMS_ASSET} has no checksum for {target_asset}"))?;

        let mut download_resp = client
            .get(&binary_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?;

        if let Some(parent) = binary_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        // Downloaded next to the binary and swapped in only once its checksum matches: an interrupted or
        // tampered download never replaces a working yt-dlp.
        let partial_path = binary_path.with_extension("download");
        let written: Result<String, String> = async {
            let mut file = File::create(&partial_path).map_err(|e| e.to_string())?;
            let mut hasher = Sha256::new();
            while let Some(chunk) = download_resp.chunk().await.map_err(|e| e.to_string())? {
                hasher.update(&chunk);
                file.write_all(&chunk).map_err(|e| e.to_string())?;
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(to_hex(&hasher.finalize()))
        }
        .await;
        let verified = written.and_then(|actual| {
            if actual == expected {
                Ok(())
            } else {
                Err(format!("yt-dlp checksum mismatch: expected {expected}, got {actual}"))
            }
        });
        if let Err(e) = verified {
            let _ = fs::remove_file(&partial_path);
            return Err(e);
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&partial_path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&partial_path, perms).map_err(|e| e.to_string())?;
        }

        if let Err(e) = fs::rename(&partial_path, &binary_path) {
            // e.g. the old yt-dlp is running (Windows locks it): keep it and try again on the next start.
            let _ = fs::remove_file(&partial_path);
            return Err(format!("cannot replace yt-dlp: {e}"));
        }

        fs::write(&version_file, &latest_version).map_err(|e| e.to_string())?;
        println!("[ytdlp] Update complete!");
    }
    Ok(())
}

pub async fn execute(app: &AppHandle, args: &[&str]) -> Result<String, String> {
    let now = Instant::now();
    println!(
        "[{:.3}s][ytdlp::execute] Executing args: {:?}",
        now.elapsed().as_secs_f32(),
        args
    );

    let binary_path = get_path(app);
    if !binary_path.exists() {
        return Err(
            "yt-dlp binary is missing. Wait for update loop to finish or restart app.".to_string(),
        );
    }
    println!("found binary {binary_path:?}");

    let process_start = Instant::now();
    let mut cmd = Command::new(binary_path);
    prepare_command(&mut cmd, app);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const FLAGS: u32 = 0x08000000 | 0x00004000; //  CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        cmd.creation_flags(FLAGS);
    }

    let output = cmd.output().await.map_err(|e| e.to_string())?;

    let duration = process_start.elapsed().as_secs_f32();
    println!(
        "[{:.3}s][ytdlp::execute] Process terminated with status: {}",
        duration, output.status
    );

    if output.status.success() {
        Ok(String::from_utf8(output.stdout)
            .map_err(|e| e.to_string())?
            .trim()
            .to_string())
    } else {
        let err_msg =
            String::from_utf8(output.stderr).unwrap_or_else(|_| "Execution failed".into());
        eprintln!(
            "[ytdlp::execute ERROR] Subprocess error message: {}",
            err_msg
        );
        Err(certificate_hint(&err_msg).map_or(err_msg, str::to_string))
    }
}

// pub async fn download_track(
//     app: &AppHandle,
//     yt_url: &str,
//     output_dir: &str,
//     file_name: &str,
// ) -> Result<String, String> {
//     let output_template = format!("{}/{}.%(ext)s", output_dir, file_name);
//
//     #[rustfmt::skip]
//     let args = [
//         "-f", "bestaudio",
//         "--format-sort", "hasaud,acodec,abr,channels,asr,aext",
//         "--no-warnings",
//         "--no-check-certificates",
//         "--socket-timeout", "5",
//         "-o", &output_template,
//         yt_url,
//     ];
//
//     execute(app, &args).await
// }

const MAX_CONCURRENT_DOWNLOADS: usize = 4;

#[derive(Clone)]
struct DownloadContext {
    app: AppHandle,
    task_id: String,
    binary_path: PathBuf,
    /// The login window's session, if yt-dlp can read it (see `cookies::find`).
    cookies: crate::download::cookies::CookieSource,
    /// Where a run's private copy of a cookie file goes (see `CookieSource::args`).
    cookie_copies: PathBuf,
    /// Format the user picked for this download (see `quality::prefer_format`).
    format: Option<String>,
    temp_dir: PathBuf,
    session_id: String,
    /// Why each failed track failed (shown to the user when nothing could be downloaded).
    failures: Arc<std::sync::Mutex<Vec<String>>>,
}

/// The line of yt-dlp's stderr that says why it failed ("ERROR: [Bandcamp] 1: No video formats found!"), else the
/// last non-empty line, else the exit code. Pure.
fn failure_reason(stderr: &str, code: Option<i32>) -> String {
    if let Some(hint) = certificate_hint(stderr) {
        return hint.to_string();
    }
    let lines = stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    let error = lines.clone().rev().find(|l| l.starts_with("ERROR:"));
    match error.or_else(|| lines.last()) {
        Some(line) => line.to_string(),
        None => format!("yt-dlp exited with code {}", code.map_or("?".to_string(), |c| c.to_string())),
    }
}

/// What the user reads when yt-dlp cannot verify a site's certificate; `None` for other errors. Pure.
pub fn certificate_hint(stderr: &str) -> Option<&'static str> {
    stderr.contains("CERTIFICATE_VERIFY_FAILED").then_some(
        "The site's security certificate could not be verified. An antivirus or a proxy that inspects HTTPS \
         traffic usually causes this: allow Blacktape in it, or turn its HTTPS scanning off.",
    )
}

/// Makes yt-dlp trust the operating system's certificate store instead of its own bundle, so the certificate of an
/// antivirus or a company proxy the system already trusts is accepted, while certificates are still verified.
pub const SYSTEM_CERTIFICATES: [&str; 2] = ["--compat-options", "no-certifi"];

/// Where Linux distributions keep the system's CA bundle (the list Go's crypto/x509 searches).
const LINUX_CA_BUNDLES: [&str; 6] = [
    "/etc/ssl/certs/ca-certificates.crt",                // Debian, Ubuntu, Arch, Gentoo
    "/etc/pki/tls/certs/ca-bundle.crt",                  // Fedora, RHEL
    "/etc/ssl/ca-bundle.pem",                            // openSUSE
    "/etc/pki/tls/cacert.pem",                           // OpenELEC
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem", // CentOS, RHEL 7
    "/etc/ssl/cert.pem",                                 // Alpine
];

/// The CA bundle the standalone Linux yt-dlp should read for `SYSTEM_CERTIFICATES`: its bundled OpenSSL looks in
/// its own build's folder and finds no certificates there, so every https request fails. `None` when the user set
/// `SSL_CERT_FILE` themselves or no bundle exists.
fn linux_ca_bundle(user_set: bool, exists: impl Fn(&Path) -> bool) -> Option<&'static str> {
    if user_set {
        return None;
    }
    LINUX_CA_BUNDLES.into_iter().find(|bundle| exists(Path::new(bundle)))
}

/// What every yt-dlp process needs besides its own arguments: the app's JavaScript runtime for YouTube, ffmpeg for
/// `--extract-audio`, and on Linux the system certificates.
pub fn prepare_command(cmd: &mut Command, app: &AppHandle) {
    cmd.args(crate::download::deno::runtime_args(app));
    cmd.args(crate::download::ffmpeg::location_args(app));
    if cfg!(target_os = "linux") {
        if let Some(bundle) = linux_ca_bundle(std::env::var_os("SSL_CERT_FILE").is_some(), Path::is_file) {
            cmd.env("SSL_CERT_FILE", bundle);
        }
    }
}

/// The url handed to yt-dlp, refused unless it is an http(s) web address: a value starting with `-` would be read
/// as an option (`--exec` runs a command), and a local path or another scheme has no business there. Pure.
pub fn checked_url(url: &str) -> Result<&str, String> {
    let refused = || format!("Not a web address: {url}");
    let parsed = url::Url::parse(url).map_err(|_| refused())?;
    let web = matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some_and(|h| !h.is_empty());
    if !web || url.starts_with('-') || url.chars().any(char::is_control) {
        return Err(refused());
    }
    Ok(url)
}

/// The site a download url belongs to, from its host (a "youtube.com" elsewhere in the url does not count).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadSite {
    Youtube,
    Bandcamp,
    Soundcloud,
    Other,
}

fn download_site(url: &str) -> DownloadSite {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on("youtube.com") || on("youtu.be") {
        DownloadSite::Youtube
    } else if on("bandcamp.com") {
        DownloadSite::Bandcamp
    } else if crate::soundcloud::is_soundcloud_url(url) {
        DownloadSite::Soundcloud
    } else {
        DownloadSite::Other
    }
}

/// yt-dlp's arguments to download one track: the url is checked and always comes last, after `--`. Pure.
fn download_args(
    url: &str,
    output_template: &str,
    cookie_args: &[String],
    format: Option<&str>,
) -> Result<Vec<String>, String> {
    let url = checked_url(url)?;
    let mut args: Vec<String> = [
        "--ignore-errors",
        "--no-playlist",
        "--concurrent-fragments",
        "10",
        "--http-chunk-size",
        "10M",
        "-o",
        output_template,
    ]
    .iter()
    .chain(SYSTEM_CERTIFICATES.iter())
    .map(|s| s.to_string())
    .collect();

    match download_site(url) {
        DownloadSite::Youtube => args.extend(
            [
                "-f",
                "251/140/ba/bestaudio/ba*", // Opus ~160k or AAC 128k; else any format that has audio (age-gated videos only offer a 360p mp4)
                "--extract-audio",
                "--audio-format",
                "best",
                "--audio-quality",
                "0", // Ensures maximum VBR quality if ffmpeg runs
                "--extractor-args",
                "youtube:player-client=web_embedded,web,tv", // Fixed client to eliminate 403 / low-bitrate lockouts
            ]
            .map(String::from)
            .into_iter()
            .chain(cookie_args.iter().cloned()),
        ),
        DownloadSite::Bandcamp => args.extend(
            [
                "-f",
                "flac/alac/wav/aiff-lossless/mp3-320/mp3-v0/aac-hi/vorbis/bestaudio/best",
                "--format-sort",
                "hasaud,ext:flac:wav:alac:mp3,abr",
                "--extract-audio",
                "--audio-format",
                "best",
                "--audio-quality",
                "0",
            ]
            .map(String::from)
            .into_iter()
            .chain(cookie_args.iter().cloned()),
        ),
        DownloadSite::Soundcloud => args.extend(soundcloud_format_args()),
        DownloadSite::Other => args.extend(["-f", "bestaudio/best"].map(String::from)),
    }

    let mut args = crate::download::quality::prefer_format(args, format);
    args.push("--newline".to_string());
    args.push("--".to_string());
    args.push(url.to_string());
    Ok(args)
}

impl DownloadContext {
    fn record_failure(&self, reason: String) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.push(reason);
        }
    }

    async fn prepare(
        app: &AppHandle,
        output_dir: &str,
        session_prefix: &str,
        task_id: &str,
    ) -> Result<Self, String> {
        let binary_path = get_path(app);
        if !binary_path.exists() {
            eprintln!("[yt-dlp ERROR] Binary missing at path: {:?}", binary_path);
            return Err("yt-dlp binary is missing.".to_string());
        }

        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let session_id = format!("{}_{}", session_prefix, timestamp);

        let temp_root = PathBuf::from(output_dir).join(".temp");
        let _ = tokio::fs::create_dir_all(&temp_root).await;
        let _ = crate::utils::set_hidden(&temp_root, true);

        let temp_dir = temp_root.join(&session_id);

        // Register the path BEFORE creating it, so an abort at any later await
        // point still lets cancel_download clean the directory up.
        if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
            registry.set_temp_dir(task_id, temp_dir.clone()).await;
        }

        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
            if registry.is_cancelled(task_id).await {
                let _ = crate::utils::force_remove_dir_all(&temp_dir);
                return Err("Task cancelled".into());
            }
        }

        let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let cookies = crate::download::cookies::find(&crate::auth::youtube::profile_dir(app)?);
        let cookie_copies = crate::download::cookies::copies_dir(&app_data_dir);
        let format = match app.try_state::<crate::download::TaskRegistry>() {
            Some(registry) => registry.format(task_id).await,
            None => None,
        };

        Ok(Self {
            app: app.clone(),
            task_id: task_id.to_string(),
            binary_path,
            cookies,
            cookie_copies,
            format,
            temp_dir,
            session_id,
            failures: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }
}

/// yt-dlp format arguments for SoundCloud: progressive MP3 when SoundCloud still offers
/// it, else the best HLS stream (usually AAC 160k). Go+ previews are filtered out in
/// `download::soundcloud` before a track gets here.
pub fn soundcloud_format_args() -> Vec<String> {
    [
        "-f",
        "bestaudio[protocol^=http]/bestaudio/best",
        "--extract-audio",
        "--audio-format",
        "best",
        "--audio-quality",
        "0",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn parse_download_percent(line: &str) -> Option<i32> {
    let parts: Vec<&str> = line.split('%').collect();
    if parts.len() >= 2 {
        let before_pct = parts[0];
        let word = before_pct.split_whitespace().last()?;
        if let Ok(val) = word.parse::<f32>() {
            return Some(val.round() as i32);
        }
    }
    None
}

async fn download_track_internal(
    ctx: &DownloadContext,
    track: &TrackDownload,
    track_num: usize,
    total_tracks: usize,
) -> Option<PathBuf> {
    let output_template = ctx.temp_dir.join(format!("{}.%(ext)s", track_num));
    // `_cookie_copy` deletes this run's cookie file copy when the track is done
    let copy_path = ctx.cookie_copies.join(format!("{}_{}.txt", ctx.session_id, track_num));
    let (cookie_args, _cookie_copy) = match ctx.cookies.args(&copy_path) {
        Ok(cookies) => cookies,
        Err(err) => {
            eprintln!(
                "[yt-dlp] [{}] [Track {}/{}] Downloading without cookies, the cookie file could not be copied: {}",
                ctx.session_id, track_num, total_tracks, err
            );
            (Vec::new(), None)
        }
    };
    let args = match download_args(&track.url, &output_template.to_string_lossy(), &cookie_args, ctx.format.as_deref()) {
        Ok(args) => args,
        Err(err) => {
            eprintln!(
                "[yt-dlp ERROR] [{}] [Track {}/{}] {}",
                ctx.session_id, track_num, total_tracks, err
            );
            ctx.record_failure(err);
            return None;
        }
    };

    println!(
        "[yt-dlp] [{}] [Track {}/{}] Executing:\n  {:?} {}",
        ctx.session_id,
        track_num,
        total_tracks,
        ctx.binary_path,
        args.join(" ")
    );

    if let Some(registry) = ctx.app.try_state::<crate::download::TaskRegistry>() {
        if registry.is_cancelled(&ctx.task_id).await {
            return None;
        }
    }

    let mut cmd = Command::new(&ctx.binary_path);
    prepare_command(&mut cmd, &ctx.app);
    cmd.args(&args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            eprintln!(
                "[yt-dlp ERROR] [{}] [Track {}/{}] Process failed to execute: {}",
                ctx.session_id, track_num, total_tracks, err
            );
            return None;
        }
    };

    let pid = child.id();
    if let (Some(registry), Some(p)) = (ctx.app.try_state::<crate::download::TaskRegistry>(), pid) {
        registry.register_pid(&ctx.task_id, p).await;
        if registry.is_cancelled(&ctx.task_id).await {
            let _ = child.kill().await;
            return None;
        }
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let app_handle = ctx.app.clone();
    let task_id = ctx.task_id.clone();
    let track_title = track.title.clone();
    let track_index = track_num - 1;

    let stdout_task = tokio::spawn(async move {
        let mut lines_acc = Vec::new();
        if let Some(stdout) = stdout {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
            let mut last_pct: i32 = -1;
            while let Ok(Some(line)) = reader.next_line().await {
                if line.contains("[download]") && line.contains('%') {
                    if let Some(pct) = parse_download_percent(&line) {
                        let pct_int = pct.clamp(0, 100);
                        if pct_int != last_pct {
                            last_pct = pct_int;
                            if let Some(registry) =
                                app_handle.try_state::<crate::download::TaskRegistry>()
                            {
                                registry
                                    .update_track_progress(
                                        &task_id,
                                        track_index,
                                        &track_title,
                                        pct_int,
                                    )
                                    .await;
                            }
                            let _ = app_handle.emit(
                                "download-track-progress",
                                serde_json::json!({
                                    "task_id": task_id,
                                    "track_index": track_index,
                                    "track_title": track_title,
                                    "percent": pct_int,
                                }),
                            );
                        }
                    }
                }
                lines_acc.push(line);
            }
        }
        lines_acc.join("\n")
    });

    let stderr_task = tokio::spawn(async move {
        let mut lines_acc = Vec::new();
        if let Some(stderr) = stderr {
            let mut reader = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                lines_acc.push(line);
            }
        }
        lines_acc.join("\n")
    });

    let status = match child.wait().await {
        Ok(s) => {
            if let (Some(registry), Some(p)) =
                (ctx.app.try_state::<crate::download::TaskRegistry>(), pid)
            {
                registry.unregister_pid(&ctx.task_id, p).await;
            }
            s
        }
        Err(err) => {
            if let (Some(registry), Some(p)) =
                (ctx.app.try_state::<crate::download::TaskRegistry>(), pid)
            {
                registry.unregister_pid(&ctx.task_id, p).await;
            }
            eprintln!(
                "[yt-dlp ERROR] [{}] [Track {}/{}] Wait error: {}",
                ctx.session_id, track_num, total_tracks, err
            );
            return None;
        }
    };

    let stdout_str = stdout_task.await.unwrap_or_default();
    let stderr_str = stderr_task.await.unwrap_or_default();

    if !status.success() {
        eprintln!(
            "[yt-dlp ERROR] [{}] [Track {}/{}] Exited with code {:?}\n--- STDERR ---\n{}\n--- STDOUT ---\n{}",
            ctx.session_id,
            track_num,
            total_tracks,
            status.code(),
            stderr_str.trim(),
            stdout_str.trim()
        );
        ctx.record_failure(failure_reason(&stderr_str, status.code()));
        return None;
    }

    if !stderr_str.trim().is_empty() {
        println!(
            "[yt-dlp LOG] [{}] [Track {}/{}] Warnings/Info:\n{}",
            ctx.session_id,
            track_num,
            total_tracks,
            stderr_str.trim()
        );
    }

    let found = find_downloaded_file(&ctx.temp_dir, track_num).await;
    if found.is_none() {
        // exit code 0 but no file: yt-dlp usually printed why ("Requested format is not available")
        ctx.record_failure(failure_reason(&stderr_str, status.code()));
    }
    found
}

pub async fn download_single(
    app: &AppHandle,
    track: &TrackDownload,
    output_dir: &str,
    task_id: &str,
) -> Result<(PathBuf, TrackDownload), String> {
    let mut results = download_batch(app, std::slice::from_ref(track), output_dir, task_id).await?;

    results
        .pop()
        .ok_or_else(|| format!("Failed to download track: {}", track.title))
}

pub async fn download_batch(
    app: &AppHandle,
    tracks: &[TrackDownload],
    output_dir: &str,
    task_id: &str,
) -> Result<Vec<(PathBuf, TrackDownload)>, String> {
    let now = Instant::now();
    let ctx = Arc::new(DownloadContext::prepare(app, output_dir, "batch", task_id).await?);

    println!(
        "[yt-dlp] [{}] Starting batch of {} tracks. Output temp dir: {:?}",
        ctx.session_id,
        tracks.len(),
        ctx.temp_dir
    );

    if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
        if registry.is_cancelled(task_id).await {
            return Err("Task cancelled".into());
        }
    }

    let total_tracks = tracks.len();
    let completed_counter = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

    struct AbortOnDropSet<T: 'static> {
        inner: tokio::task::JoinSet<T>,
    }

    impl<T: 'static> AbortOnDropSet<T> {
        fn new() -> Self {
            Self {
                inner: tokio::task::JoinSet::new(),
            }
        }
    }

    impl<T: 'static> std::ops::Deref for AbortOnDropSet<T> {
        type Target = tokio::task::JoinSet<T>;
        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    impl<T: 'static> std::ops::DerefMut for AbortOnDropSet<T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner
        }
    }

    impl<T: 'static> Drop for AbortOnDropSet<T> {
        fn drop(&mut self) {
            self.inner.abort_all();
        }
    }

    let mut join_set = AbortOnDropSet::new();

    for (index, track) in tracks.iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        let completed = Arc::clone(&completed_counter);
        let track = track.clone();
        let app_handle = app.clone();
        let ctx = Arc::clone(&ctx);
        let task_id = task_id.to_string();

        join_set.spawn(async move {
            if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
                if registry.is_cancelled(&task_id).await {
                    return None;
                }
            }
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(_) => return None,
            };
            if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
                if registry.is_cancelled(&task_id).await {
                    return None;
                }
            }

            let track_num = index + 1;
            let jitter = (index % 4) * 100 + 50;
            sleep(Duration::from_millis(jitter as u64)).await;

            let downloaded_file = download_track_internal(&ctx, &track, track_num, total_tracks).await;

            if let Some(ref path) = downloaded_file {
                let current = completed.fetch_add(1, Ordering::SeqCst) + 1;
                println!(
                    "[yt-dlp SUCCESS] [{}] [Track {}/{}] Output saved to: {:?}",
                    ctx.session_id, track_num, total_tracks, path
                );

                let _ = app_handle.emit(
                    "download-task-progress",
                    ProgressEvent {
                        task_id: task_id.clone(),
                        current,
                        total: total_tracks,
                        track_title: format!("Downloaded {} of {} tracks", current, total_tracks),
                    },
                );

                let _ = app_handle.emit(
                    "download-track-progress",
                    serde_json::json!({
                        "task_id": task_id,
                        "track_index": index,
                        "track_title": track.title.clone(),
                        "percent": 100,
                    }),
                );

                if let Some(registry) =
                    app_handle.try_state::<crate::download::TaskRegistry>()
                {
                    registry
                        .update_track_progress(&task_id, index, &track.title, 100)
                        .await;
                    registry
                        .update_task_progress(
                            &task_id,
                            current,
                            total_tracks,
                            &format!(
                                "Downloaded {} of {} tracks",
                                current, total_tracks
                            ),
                        )
                        .await;
                }

                Some((path.clone(), track))
            } else {
                eprintln!(
                    "[yt-dlp ERROR] [{}] [Track {}/{}] Process succeeded, but no output file matching '{}.*' was found in {:?}",
                    ctx.session_id, track_num, total_tracks, track_num, ctx.temp_dir
                );
                None
            }
        });
    }

    let mut downloaded: Vec<(PathBuf, TrackDownload)> = Vec::new();
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Some(item)) => downloaded.push(item),
            Ok(None) => {}
            Err(join_err) => {
                if !join_err.is_cancelled() {
                    eprintln!("[yt-dlp ERROR] Task panic/join error: {}", join_err);
                }
            }
        }
    }

    println!(
        "[yt-dlp SUMMARY] Batch finished. Downloaded {}/{} tracks in {:?}",
        downloaded.len(),
        total_tracks,
        now.elapsed()
    );

    // Nothing downloaded because every track failed: tell the user why instead of finishing silently.
    if downloaded.is_empty() {
        let reasons = ctx.failures.lock().map(|f| f.clone()).unwrap_or_default();
        if !reasons.is_empty() {
            return Err(reasons.join("; "));
        }
    }

    Ok(downloaded)
}

// pub async fn download_batch_old(
//     app: &AppHandle,
//     tracks: &[TrackDownload],
//     output_dir: &str,
//     task_id: &str,
// ) -> Result<Vec<(PathBuf, TrackDownload)>, String> {
//     let now = Instant::now();
//     let binary_path = get_path(app);
//     if !binary_path.exists() {
//         eprintln!("[yt-dlp ERROR] Binary missing at path: {:?}", binary_path);
//         return Err("yt-dlp binary is missing.".to_string());
//     }
//
//     let timestamp = SystemTime::now()
//         .duration_since(std::time::UNIX_EPOCH)
//         .map_err(|e| e.to_string())?
//         .as_millis();
//     let session_id = format!("batch_{}", timestamp);
//
//     let temp_batch_dir = PathBuf::from(output_dir).join(".temp").join(&session_id);
//     tokio::fs::create_dir_all(&temp_batch_dir)
//         .await
//         .map_err(|e| e.to_string())?;
//
//     let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
//     let edge_profile_path = app_data_dir
//         .join("browser-profiles")
//         .join("EBWebView")
//         .join("Default");
//     let cookie_arg = format!("edge:{}", edge_profile_path.to_string_lossy());
//
//     println!(
//         "[yt-dlp] [{}] Starting batch of {} tracks. Output temp dir: {:?}",
//         session_id,
//         tracks.len(),
//         temp_batch_dir
//     );
//
//     let total_tracks = tracks.len();
//     let completed_counter = Arc::new(AtomicUsize::new(0));
//     let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
//
//     let mut tasks = Vec::new();
//
//     for (index, track) in tracks.iter().enumerate() {
//         let sem = Arc::clone(&semaphore);
//         let completed = Arc::clone(&completed_counter);
//         let track = track.clone();
//         let app_handle = app.clone();
//         let binary_path = binary_path.clone();
//         let temp_batch_dir = temp_batch_dir.clone();
//         let cookie_arg = cookie_arg.clone();
//         let task_id = task_id.to_string();
//         let session_id = session_id.clone();
//
//         tasks.push(tokio::spawn(async move {
//             let _permit = sem.acquire().await.unwrap();
//
//             let track_num = index + 1;
//             let jitter = (index % 4) * 100 + 50;
//             sleep(Duration::from_millis(jitter as u64)).await;
//
//             let output_template = temp_batch_dir.join(format!("{}.%(ext)s", track_num));
//             let is_youtube = track.url.contains("youtube.com") || track.url.contains("youtu.be");
//
//             let mut args = vec![
//                 "--extract-audio".to_string(),
//                 "--audio-format".to_string(), "best".to_string(),
//                 "--ignore-errors".to_string(),
//                 "--no-check-certificates".to_string(),
//                 "--no-playlist".to_string(),
//                 "--concurrent-fragments".to_string(), "10".to_string(),
//                 "--http-chunk-size".to_string(), "10M".to_string(),
//                 "-o".to_string(), output_template.to_string_lossy().to_string(),
//             ];
//
//             if is_youtube {
//                 args.extend(vec![
//                     "-f".to_string(), "ba[ext=webm]/ba[ext=m4a]/ba".to_string(),
//                     "--cookies-from-browser".to_string(), cookie_arg.clone(),
//                 ]);
//             } else {
//                 args.extend(vec![
//                     "-f".to_string(), "bestaudio/best".to_string(),
//                 ]);
//             }
//
//             args.push(track.url.clone());
//
//             println!(
//                 "[yt-dlp] [{}] [Track {}/{}] Executing:\n  {:?} {}",
//                 session_id, track_num, total_tracks, binary_path, args.join(" ")
//             );
//
//             let mut cmd = Command::new(&binary_path);
//             cmd.args(&args);
//
//             #[cfg(target_os = "windows")]
//             {
//                 const CREATE_NO_WINDOW: u32 = 0x08000000;
//                 cmd.creation_flags(CREATE_NO_WINDOW);
//             }
//
//             let output = match cmd.output().await {
//                 Ok(out) => out,
//                 Err(err) => {
//                     eprintln!(
//                         "[yt-dlp ERROR] [{}] [Track {}/{}] Process failed to execute: {}",
//                         session_id, track_num, total_tracks, err
//                     );
//                     return None;
//                 }
//             };
//
//             let stdout = String::from_utf8_lossy(&output.stdout);
//             let stderr = String::from_utf8_lossy(&output.stderr);
//
//             if !output.status.success() {
//                 eprintln!(
//                     "[yt-dlp ERROR] [{}] [Track {}/{}] Exited with code {:?}\n--- STDERR ---\n{}\n--- STDOUT ---\n{}",
//                     session_id,
//                     track_num,
//                     total_tracks,
//                     output.status.code(),
//                     stderr.trim(),
//                     stdout.trim()
//                 );
//                 return None;
//             }
//
//             if !stderr.trim().is_empty() {
//                 println!(
//                     "[yt-dlp LOG] [{}] [Track {}/{}] Warnings/Info:\n{}",
//                     session_id, track_num, total_tracks, stderr.trim()
//                 );
//             }
//
//             let downloaded_file = find_downloaded_file(&temp_batch_dir, track_num).await;
//
//             if let Some(ref path) = downloaded_file {
//                 let current = completed.fetch_add(1, Ordering::SeqCst) + 1;
//                 println!(
//                     "[yt-dlp SUCCESS] [{}] [Track {}/{}] Output saved to: {:?}",
//                     session_id, track_num, total_tracks, path
//                 );
//
//                 let _ = app_handle.emit(
//                     "download-task-progress",
//                     ProgressEvent {
//                         task_id,
//                         current,
//                         total: total_tracks,
//                         track_title: format!("Downloaded {} of {} tracks", current, total_tracks),
//                     },
//                 );
//
//                 downloaded_file.map(|p| (p, track))
//             } else {
//                 eprintln!(
//                     "[yt-dlp ERROR] [{}] [Track {}/{}] Process succeeded, but no output file matching '{}.*' was found in {:?}",
//                     session_id, track_num, total_tracks, track_num, temp_batch_dir
//                 );
//                 None
//             }
//         }));
//     }
//
//     let results = futures::future::join_all(tasks).await;
//     let downloaded: Vec<(PathBuf, TrackDownload)> = results
//         .into_iter()
//         .filter_map(|r| match r {
//             Ok(opt) => opt,
//             Err(join_err) => {
//                 eprintln!("[yt-dlp ERROR] Task panic/join error: {}", join_err);
//                 None
//             }
//         })
//         .collect();
//
//     println!(
//         "[yt-dlp SUMMARY] Batch finished. Downloaded {}/{} tracks in {:?}",
//         downloaded.len(),
//         total_tracks,
//         now.elapsed()
//     );
//
//     Ok(downloaded)
// }

/// Helper function to match track index to the resulting file on disk
async fn find_downloaded_file(dir: &Path, index: usize) -> Option<PathBuf> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem == index.to_string() {
                let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
                if ext != "part" && ext != "ytdl" {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod failure_reason_tests {
    use super::failure_reason;

    #[test]
    fn picks_the_error_line() {
        let stderr = "WARNING: x
ERROR: [Bandcamp] 1: No video formats found!; please report
";
        assert!(failure_reason(stderr, Some(1)).contains("No video formats found"));
    }

    #[test]
    fn falls_back_to_last_line_then_exit_code() {
        assert_eq!(failure_reason("a
b
", Some(1)), "b");
        assert_eq!(failure_reason("  
", Some(2)), "yt-dlp exited with code 2");
    }
}

#[cfg(test)]
mod hardening_tests {
    use super::*;

    const YT: &str = "https://www.youtube.com/watch?v=abc";
    const COOKIES: &str = "chrome";

    fn cookie_args() -> Vec<String> {
        vec!["--cookies-from-browser".to_string(), COOKIES.to_string()]
    }
    const TEMPLATE: &str = "out/%(id)s.%(ext)s";
    const SSL_STDERR: &str =
        "ERROR: unable to download: [SSL: CERTIFICATE_VERIFY_FAILED] certificate verify failed (_ssl.c:1006)";

    /// A 64-character hash made of one repeated hex digit.
    fn hash(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn checked_url_accepts_web_addresses_and_returns_them_unchanged() {
        assert_eq!(checked_url(YT), Ok(YT));
        assert_eq!(
            checked_url("http://x.bandcamp.com/track/a"),
            Ok("http://x.bandcamp.com/track/a")
        );
    }

    #[test]
    fn checked_url_rejects_options_local_paths_other_schemes_and_control_chars() {
        for bad in [
            "--exec=calc",
            "-x",
            "file:///C:/a.mp3",
            "C:\\music\\a.mp3",
            "ftp://x.com/a",
            "https://x.com/a\nhttps://y.com",
            "",
            "https://",
        ] {
            assert!(checked_url(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn download_site_is_chosen_by_host_only() {
        assert_eq!(download_site(YT), DownloadSite::Youtube);
        assert_eq!(download_site("https://music.youtube.com/watch?v=abc"), DownloadSite::Youtube);
        assert_eq!(download_site("https://youtu.be/abc"), DownloadSite::Youtube);
        assert_eq!(download_site("https://artist.bandcamp.com/track/a"), DownloadSite::Bandcamp);
        assert_eq!(download_site("https://soundcloud.com/artist/track"), DownloadSite::Soundcloud);
        assert_eq!(download_site("https://evil.com/?youtube.com"), DownloadSite::Other);
        assert_eq!(download_site("https://notyoutube.com/x"), DownloadSite::Other);
    }

    #[test]
    fn youtube_args_end_with_separator_and_url_and_trust_the_system_store() {
        let args = download_args(YT, TEMPLATE, &cookie_args(), None).unwrap();
        let tail: Vec<&str> = args[args.len() - 3..].iter().map(String::as_str).collect();
        assert_eq!(tail, ["--newline", "--", YT]);

        let compat = args.iter().position(|a| a == "--compat-options").unwrap();
        assert_eq!(args[compat + 1], "no-certifi");
        assert!(!args.iter().any(|a| a == "--no-check-certificates"));

        let cookies = args.iter().position(|a| a == "--cookies-from-browser").unwrap();
        assert_eq!(args[cookies + 1], COOKIES);
    }

    #[test]
    fn non_youtube_urls_never_get_browser_cookies() {
        let args = download_args("https://evil.com/?youtube.com", TEMPLATE, &cookie_args(), None).unwrap();
        assert!(!args.iter().any(|a| a == "--cookies-from-browser"));
    }

    #[test]
    fn downloads_without_a_session_pass_no_cookie_options() {
        for url in [YT, "https://artist.bandcamp.com/track/a"] {
            let args = download_args(url, TEMPLATE, &[], None).unwrap();
            assert!(
                !args.iter().any(|a| a.starts_with("--cookies")),
                "cookie option without a session for {url}: {args:?}"
            );
        }
    }

    #[test]
    fn a_cookie_file_is_passed_before_the_separator() {
        let cookies = vec!["--cookies".to_string(), "/data/cookie-copies/1.txt".to_string()];
        let args = download_args("https://artist.bandcamp.com/track/a", TEMPLATE, &cookies, None).unwrap();
        let at = args.iter().position(|a| a == "--cookies").unwrap();
        assert_eq!(args[at + 1], "/data/cookie-copies/1.txt");
        assert!(at < args.iter().position(|a| a == "--").unwrap());
    }

    #[test]
    fn download_args_refuses_option_like_urls() {
        assert!(download_args("--exec=calc", TEMPLATE, &cookie_args(), None).is_err());
    }

    #[test]
    fn certificate_hint_only_for_certificate_verify_failures() {
        assert!(certificate_hint(SSL_STDERR).is_some());
        assert!(certificate_hint("ERROR: [Bandcamp] 1: No video formats found!").is_none());
    }

    #[test]
    fn failure_reason_shows_the_certificate_hint_for_ssl_errors() {
        assert_eq!(failure_reason(SSL_STDERR, Some(1)), certificate_hint(SSL_STDERR).unwrap());
    }

    #[test]
    fn expected_sha256_matches_the_exact_asset_name() {
        let exe = hash('a');
        let bin = hash('b');
        let mac = hash('c');
        let sums = format!("{exe}  yt-dlp.exe\n{bin}  yt-dlp\n{mac} *yt-dlp_macos\n");
        assert_eq!(expected_sha256(&sums, "yt-dlp.exe"), Some(exe.clone()));
        assert_eq!(expected_sha256(&sums, "yt-dlp"), Some(bin));
        assert_eq!(expected_sha256(&sums, "yt-dlp_macos"), Some(mac));
    }

    #[test]
    fn expected_sha256_does_not_match_a_longer_name() {
        let sums = format!("{}  yt-dlp.exe\n", hash('a'));
        assert_eq!(expected_sha256(&sums, "yt-dlp"), None);
    }

    #[test]
    fn expected_sha256_returns_lowercase_for_uppercase_hex() {
        let upper = "ABCDEF0123456789".repeat(4);
        let sums = format!("{upper}  yt-dlp\n");
        assert_eq!(expected_sha256(&sums, "yt-dlp"), Some(upper.to_ascii_lowercase()));
    }

    #[test]
    fn expected_sha256_ignores_malformed_hashes_and_missing_assets() {
        let short = "a".repeat(63);
        let non_hex = format!("{}g", "a".repeat(63));
        let sums = format!("{short}  yt-dlp\n{non_hex}  yt-dlp.exe\n");
        assert_eq!(expected_sha256(&sums, "yt-dlp"), None);
        assert_eq!(expected_sha256(&sums, "yt-dlp.exe"), None);

        let only_exe = format!("{}  yt-dlp.exe\n", hash('a'));
        assert_eq!(expected_sha256(&only_exe, "yt-dlp_macos"), None);
    }

    #[test]
    fn linux_yt_dlp_reads_the_first_system_ca_bundle_that_exists() {
        let fedora = |p: &Path| p == Path::new("/etc/pki/tls/certs/ca-bundle.crt");
        assert_eq!(linux_ca_bundle(false, fedora), Some("/etc/pki/tls/certs/ca-bundle.crt"));
        assert_eq!(linux_ca_bundle(false, |_| true), Some("/etc/ssl/certs/ca-certificates.crt"));
        assert_eq!(linux_ca_bundle(false, |_| false), None);
    }

    #[test]
    fn a_ca_bundle_the_user_set_is_kept() {
        assert_eq!(linux_ca_bundle(true, |_| true), None);
    }

    #[test]
    fn linux_gets_the_standalone_build_that_can_impersonate() {
        assert_eq!(release_asset("linux", "x86_64"), "yt-dlp_linux");
        assert_eq!(release_asset("linux", "aarch64"), "yt-dlp_linux_aarch64");
        assert_eq!(release_asset("linux", "arm"), "yt-dlp_linux_armv7l");
        assert_eq!(release_asset("windows", "x86_64"), "yt-dlp.exe");
        assert_eq!(release_asset("macos", "aarch64"), "yt-dlp_macos");
    }

    #[test]
    fn a_binary_installed_from_another_asset_is_replaced() {
        // installs before the asset was recorded wrote only the tag
        assert_ne!(installed_marker("2026.10.01", "yt-dlp_linux"), "2026.10.01");
        assert_ne!(installed_marker("2026.10.01", "yt-dlp_linux"), installed_marker("2026.10.01", "yt-dlp"));
        assert_eq!(installed_marker(" 2026.10.01\n", "yt-dlp_linux"), "2026.10.01 yt-dlp_linux");
    }

    #[test]
    fn to_hex_is_lowercase_and_zero_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xab]), "000fab");
    }
}

#[cfg(test)]
mod format_choice_tests {
    use super::*;

    const YT: &str = "https://www.youtube.com/watch?v=abc";
    const BANDCAMP: &str = "https://artist.bandcamp.com/track/a";
    const SOUNDCLOUD: &str = "https://soundcloud.com/artist/track";
    const COOKIES: &str = "chrome";

    fn cookie_args() -> Vec<String> {
        vec!["--cookies-from-browser".to_string(), COOKIES.to_string()]
    }
    const TEMPLATE: &str = "out/%(id)s.%(ext)s";

    /// The value that follows the first `-f`.
    fn selector(args: &[String]) -> &str {
        let i = args.iter().position(|a| a == "-f").expect("no -f");
        &args[i + 1]
    }

    #[test]
    fn chosen_format_is_tried_first_for_youtube() {
        let args = download_args(YT, TEMPLATE, &cookie_args(), Some("140")).unwrap();
        assert_eq!(selector(&args), "140/251/140/ba/bestaudio/ba*");
    }

    #[test]
    fn chosen_format_is_tried_first_for_bandcamp() {
        let args = download_args(BANDCAMP, TEMPLATE, &cookie_args(), Some("mp3-320")).unwrap();
        assert_eq!(
            selector(&args),
            "mp3-320/flac/alac/wav/aiff-lossless/mp3-320/mp3-v0/aac-hi/vorbis/bestaudio/best"
        );
    }

    #[test]
    fn chosen_format_is_tried_first_for_soundcloud() {
        let args = download_args(SOUNDCLOUD, TEMPLATE, &cookie_args(), Some("hls_aac_160k")).unwrap();
        assert_eq!(selector(&args), "hls_aac_160k/bestaudio[protocol^=http]/bestaudio/best");
    }

    #[test]
    fn chosen_format_is_tried_first_for_other_sites() {
        let args = download_args("https://example.test/a.mp3", TEMPLATE, &cookie_args(), Some("ab-1")).unwrap();
        assert_eq!(selector(&args), "ab-1/bestaudio/best");
    }

    #[test]
    fn no_format_keeps_the_platform_selector() {
        let args = download_args(YT, TEMPLATE, &cookie_args(), None).unwrap();
        assert_eq!(selector(&args), "251/140/ba/bestaudio/ba*");
    }

    #[test]
    fn unsafe_format_ids_leave_the_args_unchanged() {
        let plain = download_args(YT, TEMPLATE, &cookie_args(), None).unwrap();
        for bad in ["251/best", "a+b", ""] {
            let args = download_args(YT, TEMPLATE, &cookie_args(), Some(bad)).unwrap();
            assert_eq!(args, plain, "changed for {bad:?}");
        }
    }

    #[test]
    fn format_choice_keeps_the_url_last_after_the_separator() {
        let args = download_args(YT, TEMPLATE, &cookie_args(), Some("140")).unwrap();
        let tail: Vec<&str> = args[args.len() - 3..].iter().map(String::as_str).collect();
        assert_eq!(tail, ["--newline", "--", YT]);
    }
}
