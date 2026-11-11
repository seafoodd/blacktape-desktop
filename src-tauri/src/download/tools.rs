//! What the helper programs behind downloads and previews (yt-dlp, deno, ffmpeg) share: their setup state for the
//! UI, downloads that are retried and verified while they stream to disk, swapping a new binary in even while the
//! old one runs, and how often GitHub is asked for a newer release.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Mutex};

/// The event that carries `ToolsStatus` to every window whenever it changes.
pub const STATUS_EVENT: &str = "tools-status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    YtDlp,
    Deno,
    Ffmpeg,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::YtDlp, Tool::Deno, Tool::Ffmpeg];

    pub fn label(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Deno => "deno",
            Tool::Ffmpeg => "ffmpeg",
        }
    }

    /// How long a check that found the latest release is trusted. yt-dlp follows site changes and releases often;
    /// deno and ffmpeg rarely matter between releases.
    pub fn check_interval(self) -> Duration {
        const DAY: u64 = 24 * 60 * 60;
        match self {
            Tool::YtDlp => Duration::from_secs(DAY),
            Tool::Deno | Tool::Ffmpeg => Duration::from_secs(7 * DAY),
        }
    }

    fn index(self) -> usize {
        match self {
            Tool::YtDlp => 0,
            Tool::Deno => 1,
            Tool::Ffmpeg => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Looking at what is installed or asking GitHub for the latest release.
    Checking,
    Downloading,
    Ready,
    /// The last setup or update failed; `error` says why.
    Failed,
    /// No build exists for this OS and CPU.
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Installed by the user; the app neither downloads nor updates it.
    System,
    /// The app's own copy in its data folder.
    App,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub phase: Phase,
    /// A working copy is on disk: jobs can run, also while a newer one downloads or after an update failed.
    pub usable: bool,
    pub version: Option<String>,
    pub source: Option<Source>,
    /// Bytes on disk of the app's copy.
    pub size: Option<u64>,
    /// Progress of the running download.
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

impl Default for ToolStatus {
    fn default() -> Self {
        ToolStatus {
            phase: Phase::Checking,
            usable: false,
            version: None,
            source: None,
            size: None,
            downloaded: 0,
            total: None,
            error: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    /// Grows with every change, so a window keeps the newest of a snapshot and events that cross on the way.
    pub seq: u64,
    pub yt_dlp: ToolStatus,
    pub deno: ToolStatus,
    pub ffmpeg: ToolStatus,
}

impl ToolsStatus {
    pub fn get(&self, tool: Tool) -> &ToolStatus {
        match tool {
            Tool::YtDlp => &self.yt_dlp,
            Tool::Deno => &self.deno,
            Tool::Ffmpeg => &self.ffmpeg,
        }
    }

    fn get_mut(&mut self, tool: Tool) -> &mut ToolStatus {
        match tool {
            Tool::YtDlp => &mut self.yt_dlp,
            Tool::Deno => &mut self.deno,
            Tool::Ffmpeg => &mut self.ffmpeg,
        }
    }
}

fn registry() -> &'static watch::Sender<ToolsStatus> {
    static STATUS: OnceLock<watch::Sender<ToolsStatus>> = OnceLock::new();
    STATUS.get_or_init(|| watch::Sender::new(ToolsStatus::default()))
}

pub fn snapshot() -> ToolsStatus {
    registry().borrow().clone()
}

/// Changes one tool's status and tells the windows.
pub fn update(app: &AppHandle, tool: Tool, change: impl FnOnce(&mut ToolStatus)) {
    // emitted while the status is locked, so the windows get the snapshots in order
    registry().send_modify(|all| {
        change(all.get_mut(tool));
        all.seq += 1;
        let _ = app.emit(STATUS_EVENT, &*all);
    });
}

/// Records a working copy found on disk before anything is downloaded, so jobs need not wait for GitHub.
pub fn found(app: &AppHandle, tool: Tool, version: Option<String>, source: Source, files: &[PathBuf]) {
    let size = (source == Source::App).then(|| size_of(files));
    update(app, tool, |s| {
        s.usable = true;
        s.version = version;
        s.source = Some(source);
        s.size = size;
    });
}

/// Records a finished check: `tool` is installed and as new as the app wants it.
pub fn ready(app: &AppHandle, tool: Tool, version: Option<String>, source: Source, files: &[PathBuf]) {
    let size = (source == Source::App).then(|| size_of(files));
    update(app, tool, |s| {
        *s = ToolStatus {
            phase: Phase::Ready,
            usable: true,
            version,
            source: Some(source),
            size,
            ..ToolStatus::default()
        };
    });
}

pub fn unsupported(app: &AppHandle, tool: Tool) {
    update(app, tool, |s| *s = ToolStatus { phase: Phase::Unsupported, ..ToolStatus::default() });
}

fn size_of(files: &[PathBuf]) -> u64 {
    files.iter().filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum()
}

/// The version part of a `.version` marker ("2026.10.26 yt-dlp.exe" -> "2026.10.26"). Pure.
pub fn version_of_marker(marker: &str) -> Option<String> {
    marker.split_whitespace().next().map(|v| v.trim_start_matches('v').to_string())
}

/// Whether waiting longer for `status` would change anything: a copy is installed, or setup failed or cannot
/// happen. Pure.
fn settled(status: &ToolStatus) -> bool {
    status.usable || matches!(status.phase, Phase::Failed | Phase::Unsupported)
}

/// Whether any of `tools` is still being set up for the first time. Pure.
pub fn setting_up(status: &ToolsStatus, tools: &[Tool]) -> bool {
    tools.iter().any(|t| !settled(status.get(*t)))
}

/// Whether yt-dlp, which every job needs, cannot be used: waiting for the other tools would not help. Pure.
fn yt_dlp_failed(status: &ToolsStatus) -> bool {
    let s = status.get(Tool::YtDlp);
    !s.usable && matches!(s.phase, Phase::Failed | Phase::Unsupported)
}

/// Why a job that needs `tools` cannot start with what is installed, `None` when it can. Only yt-dlp is required:
/// without deno or ffmpeg some sites still work, and yt-dlp then names what it misses. Pure.
fn blocking_error(status: &ToolsStatus, tools: &[Tool]) -> Option<String> {
    if !tools.contains(&Tool::YtDlp) {
        return None;
    }
    let s = status.get(Tool::YtDlp);
    if s.usable {
        return None;
    }
    Some(match s.phase {
        Phase::Failed => format!(
            "The downloader (yt-dlp) could not be installed: {}. Use Retry in Settings → Downloader.",
            s.error.as_deref().unwrap_or("unknown error")
        ),
        Phase::Unsupported => "The downloader (yt-dlp) has no build for this computer.".to_string(),
        _ => "The downloader (yt-dlp) is still being set up. Try again in a moment.".to_string(),
    })
}

/// How long a job waits for a first-time setup before it gives up: the first install downloads about 100 MB.
pub const SETUP_WAIT: Duration = Duration::from_secs(10 * 60);

/// How long a preview or a format lookup waits: someone is looking at a spinner.
pub const PREVIEW_WAIT: Duration = Duration::from_secs(2 * 60);

/// Waits until each of `tools` is installed or has failed (at most `timeout`), then says whether the job can run.
pub async fn wait_for(tools: &[Tool], timeout: Duration) -> Result<(), String> {
    let mut rx = registry().subscribe();
    let done = |s: &ToolsStatus| !setting_up(s, tools) || yt_dlp_failed(s);
    let _ = tokio::time::timeout(timeout, rx.wait_for(done)).await;
    blocking_error(&snapshot(), tools).map_or(Ok(()), Err)
}

// ---------------------------------------------------------------------------------------------------------------
// Downloads

pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("blacktape-desktop")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

/// One failed attempt: `retry` when trying again may help (network down, timeout, a 5xx or 429 from the server).
pub struct Failure {
    pub message: String,
    pub retry: bool,
}

impl Failure {
    fn fatal(message: impl Into<String>) -> Self {
        Failure { message: message.into(), retry: false }
    }
}

impl From<reqwest::Error> for Failure {
    fn from(e: reqwest::Error) -> Self {
        let retry = match e.status() {
            Some(status) => status.is_server_error() || status.as_u16() == 429,
            // a connection lost while the body streams is reported as a decode error
            None => e.is_timeout() || e.is_connect() || e.is_body() || e.is_request() || e.is_decode(),
        };
        Failure { message: e.to_string(), retry }
    }
}

const ATTEMPTS: u32 = 3;

/// The pause before attempt `n + 1` (n counts from 1): 1 s, then 3 s. Pure.
fn backoff(n: u32) -> Duration {
    Duration::from_secs(3u64.pow(n.saturating_sub(1)))
}

/// Runs `attempt` up to `attempts` times while it fails with a retryable error, pausing `pause(n)` between them.
async fn retrying<T, F, Fut>(attempts: u32, pause: impl Fn(u32) -> Duration, mut attempt: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let mut n = 1;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(f) if f.retry && n < attempts => {
                eprintln!("[tools] attempt {n} failed, retrying: {}", f.message);
                tokio::time::sleep(pause(n)).await;
                n += 1;
            }
            Err(f) => return Err(f.message),
        }
    }
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, Failure> {
    Ok(client.get(url).send().await?.error_for_status()?)
}

pub async fn get_json<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T, String> {
    retrying(ATTEMPTS, backoff, || async { Ok(get(client, url).await?.json::<T>().await?) }).await
}

pub async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    retrying(ATTEMPTS, backoff, || async { Ok(get(client, url).await?.text().await?) }).await
}

/// Refuses a download url that is not https under `prefix` (a release folder on github.com). Pure.
pub fn check_url(url: &str, prefix: &str) -> Result<(), String> {
    // compared after parsing, which resolves "%2e%2e" and ".." segments the way the request will
    let place = |u: &url::Url| (u.scheme().to_string(), u.host_str().map(str::to_string), u.path().to_string());
    let allowed = match (url::Url::parse(url), url::Url::parse(prefix)) {
        (Ok(u), Ok(p)) => {
            let ((scheme, host, path), (_, prefix_host, prefix_path)) = (place(&u), place(&p));
            scheme == "https" && prefix.ends_with('/') && host == prefix_host && path.starts_with(&prefix_path)
        }
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(format!("unexpected download url {url}"))
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads `url` (which must lie under `prefix`) into `dest`, retried on network errors, and keeps it only when
/// its SHA-256 is `expected`. Progress goes to `tool`'s status; `dest` is removed on failure.
pub async fn download(
    app: &AppHandle,
    tool: Tool,
    client: &reqwest::Client,
    url: &str,
    prefix: &str,
    expected: &str,
    dest: &Path,
) -> Result<(), String> {
    check_url(url, prefix)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    update(app, tool, |s| {
        s.phase = Phase::Downloading;
        s.downloaded = 0;
        s.total = None;
        s.error = None;
    });
    let result = retrying(ATTEMPTS, backoff, || download_once(app, tool, client, url, expected, dest)).await;
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

async fn download_once(
    app: &AppHandle,
    tool: Tool,
    client: &reqwest::Client,
    url: &str,
    expected: &str,
    dest: &Path,
) -> Result<(), Failure> {
    let mut response = get(client, url).await?;
    let total = response.content_length();
    let io = |e: std::io::Error| Failure::fatal(format!("cannot write {}: {e}", dest.display()));
    let mut file = tokio::fs::File::create(dest).await.map_err(io)?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0u64;
    let mut last_report = Instant::now();
    update(app, tool, |s| {
        s.downloaded = 0;
        s.total = total;
    });
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(io)?;
        downloaded += chunk.len() as u64;
        if last_report.elapsed() >= Duration::from_millis(200) {
            last_report = Instant::now();
            update(app, tool, |s| s.downloaded = downloaded);
        }
    }
    file.sync_all().await.map_err(io)?;
    update(app, tool, |s| s.downloaded = downloaded);
    let actual = to_hex(&hasher.finalize());
    if actual != expected {
        return Err(Failure::fatal(format!(
            "{} checksum mismatch: expected {expected}, got {actual}",
            tool.label()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Installing

/// Where an old binary waits to be deleted after a new one took its place: `yt-dlp.exe` -> `yt-dlp.exe.old`. Pure.
fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Puts the finished `partial` file in place of `target`. Windows does not let a running program be overwritten
/// but does let it be renamed: the old binary then moves to `<name>.old`, which `remove_leftovers` deletes on a
/// later start. `partial` is removed when this fails.
pub fn swap_in(partial: &Path, target: &Path) -> Result<(), String> {
    let result = swap(partial, target);
    if result.is_err() {
        let _ = fs::remove_file(partial);
    }
    result
}

fn swap(partial: &Path, target: &Path) -> Result<(), String> {
    make_executable(partial).map_err(|e| e.to_string())?;
    let first = match fs::rename(partial, target) {
        Ok(()) => return Ok(()),
        Err(e) if target.exists() => e,
        Err(e) => return Err(format!("cannot install {}: {e}", target.display())),
    };
    let old = old_path(target);
    let _ = fs::remove_file(&old);
    fs::rename(target, &old).map_err(|e| format!("cannot replace {} ({first}): {e}", target.display()))?;
    if let Err(e) = fs::rename(partial, target) {
        let _ = fs::rename(&old, target);
        return Err(format!("cannot replace {}: {e}", target.display()));
    }
    let _ = fs::remove_file(&old); // fails while the old one still runs: removed on a later start
    Ok(())
}

/// Deletes what an earlier run left in `dir`: old binaries swapped out while they ran, and unfinished downloads.
pub fn remove_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let leftover = path.extension().is_some_and(|e| e == "old" || e == "download");
        if leftover && path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// When to check

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// When the stamp file says GitHub was last asked, in unix seconds.
fn last_checked(stamp: &Path) -> Option<u64> {
    fs::read_to_string(stamp).ok()?.trim().parse().ok()
}

/// Whether GitHub should be asked again: never checked, the interval has passed, or the clock went back. Pure.
fn due(last: Option<u64>, now: u64, interval: Duration) -> bool {
    match last {
        Some(last) => now < last || now - last >= interval.as_secs(),
        None => true,
    }
}

pub fn check_due(tool: Tool, stamp: &Path) -> bool {
    due(last_checked(stamp), unix_now(), tool.check_interval())
}

/// Notes that GitHub was asked just now and the installed copy is current.
pub fn mark_checked(stamp: &Path) {
    let _ = fs::write(stamp, unix_now().to_string());
}

/// Whether yt-dlp's error looks like a site change that a newer yt-dlp fixes. Pure.
pub fn looks_outdated(stderr: &str) -> bool {
    const SIGNS: [&str; 5] = [
        "confirm you are on the latest version",
        "nsig extraction failed",
        "signature extraction failed",
        "unable to extract",
        "requested format is not available",
    ];
    let lower = stderr.to_ascii_lowercase();
    SIGNS.iter().any(|s| lower.contains(s))
}

/// At most one early yt-dlp check per hour, however many downloads fail.
const FAILURE_CHECK_GAP: Duration = Duration::from_secs(60 * 60);

/// After yt-dlp failed with `stderr`: looks for a newer yt-dlp now instead of waiting a day, when the error
/// suggests one would help.
pub fn after_failure(app: &AppHandle, stderr: &str) {
    if !looks_outdated(stderr) {
        return;
    }
    // counted from the last attempt, not the last success: while GitHub is unreachable, failures must not each
    // start a check
    static LAST_ATTEMPT: AtomicU64 = AtomicU64::new(0);
    let now = unix_now();
    let stamp = crate::download::ytdlp::stamp_path(app);
    let last = last_checked(&stamp).unwrap_or(0).max(LAST_ATTEMPT.load(Ordering::Relaxed));
    if due(Some(last), now, FAILURE_CHECK_GAP) {
        LAST_ATTEMPT.store(now, Ordering::Relaxed);
        println!("[tools] yt-dlp failed in a way an update may fix; checking for one");
        spawn_check(app.clone(), Tool::YtDlp, true);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Running the checks

fn lock(tool: Tool) -> &'static Mutex<()> {
    static LOCKS: OnceLock<[Mutex<()>; 3]> = OnceLock::new();
    &LOCKS.get_or_init(|| [Mutex::new(()), Mutex::new(()), Mutex::new(())])[tool.index()]
}

/// Sets up or updates `tool`; `force` skips the wait between checks. Does nothing while a check of it runs.
pub async fn check(app: AppHandle, tool: Tool, force: bool) {
    let Ok(_running) = lock(tool).try_lock() else { return };
    update(&app, tool, |s| {
        s.phase = Phase::Checking;
        s.error = None;
    });
    let result = match tool {
        Tool::YtDlp => crate::download::ytdlp::check_and_update(&app, force).await,
        Tool::Deno => crate::download::deno::check_and_update(&app, force).await,
        Tool::Ffmpeg => crate::download::ffmpeg::check_and_update(&app, force).await,
    };
    if let Err(e) = result {
        eprintln!("[tools] {} setup failed: {e}", tool.label());
        // a swap that failed half-way can leave no binary behind
        let still_usable = installed(&app, tool);
        update(&app, tool, |s| {
            s.phase = Phase::Failed;
            s.usable &= still_usable;
            s.error = Some(e);
        });
    }
}

/// Whether a copy of `tool` that jobs can run is on disk.
fn installed(app: &AppHandle, tool: Tool) -> bool {
    match tool {
        Tool::YtDlp => crate::download::ytdlp::get_path(app).is_file(),
        Tool::Deno => !crate::download::deno::runtime_args(app).is_empty(),
        Tool::Ffmpeg => !crate::download::ffmpeg::location_args(app).is_empty(),
    }
}

pub fn spawn_check(app: AppHandle, tool: Tool, force: bool) {
    tauri::async_runtime::spawn(check(app, tool, force));
}

/// At app start: clears leftovers of earlier runs and sets up or updates every tool in the background.
pub fn start(app: &AppHandle, dirs: &[PathBuf]) {
    for dir in dirs {
        remove_leftovers(dir);
    }
    for tool in Tool::ALL {
        spawn_check(app.clone(), tool, false);
    }
}

/// Every tool's setup state, for a window that opens after the `tools-status` events it missed.
#[tauri::command]
pub fn get_tools_status() -> ToolsStatus {
    snapshot()
}

/// Asks GitHub for newer releases now and retries setups that failed ("Check for updates" and "Retry").
#[tauri::command]
pub fn check_tools(app: AppHandle) {
    for tool in Tool::ALL {
        spawn_check(app.clone(), tool, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn status(phase: Phase, usable: bool) -> ToolStatus {
        ToolStatus { phase, usable, ..ToolStatus::default() }
    }

    fn all(yt_dlp: ToolStatus) -> ToolsStatus {
        ToolsStatus { seq: 0, yt_dlp, deno: status(Phase::Ready, true), ffmpeg: status(Phase::Ready, true) }
    }

    #[test]
    fn a_tool_is_settled_once_usable_failed_or_unsupported() {
        assert!(!settled(&status(Phase::Checking, false)));
        assert!(!settled(&status(Phase::Downloading, false)));
        assert!(settled(&status(Phase::Downloading, true)));
        assert!(settled(&status(Phase::Checking, true)));
        assert!(settled(&status(Phase::Failed, false)));
        assert!(settled(&status(Phase::Unsupported, false)));
    }

    #[test]
    fn setting_up_looks_only_at_the_tools_asked_about() {
        let mut s = all(status(Phase::Ready, true));
        s.ffmpeg = status(Phase::Downloading, false);
        assert!(!setting_up(&s, &[Tool::YtDlp, Tool::Deno]));
        assert!(setting_up(&s, &[Tool::YtDlp, Tool::Ffmpeg]));
    }

    #[test]
    fn only_a_missing_yt_dlp_blocks_a_job() {
        let mut s = all(status(Phase::Ready, true));
        s.deno = status(Phase::Failed, false);
        s.ffmpeg = status(Phase::Unsupported, false);
        assert_eq!(blocking_error(&s, &Tool::ALL), None);
        assert_eq!(blocking_error(&all(status(Phase::Failed, false)), &[Tool::Deno]), None);
    }

    #[test]
    fn a_missing_yt_dlp_explains_itself() {
        let mut failed = status(Phase::Failed, false);
        failed.error = Some("offline".into());
        let msg = blocking_error(&all(failed), &[Tool::YtDlp]).unwrap();
        assert!(msg.contains("offline") && msg.contains("Retry"), "{msg}");
        let msg = blocking_error(&all(status(Phase::Downloading, false)), &[Tool::YtDlp]).unwrap();
        assert!(msg.contains("still being set up"), "{msg}");
        // an older copy keeps working while its update failed
        assert_eq!(blocking_error(&all(status(Phase::Failed, true)), &[Tool::YtDlp]), None);
    }

    #[test]
    fn a_failed_yt_dlp_ends_the_wait_for_the_other_tools() {
        let mut s = all(status(Phase::Failed, false));
        s.ffmpeg = status(Phase::Downloading, false);
        assert!(yt_dlp_failed(&s));
        assert!(!yt_dlp_failed(&all(status(Phase::Failed, true))), "an older copy still works");
        assert!(!yt_dlp_failed(&all(status(Phase::Downloading, false))));
    }

    #[test]
    fn version_is_the_first_word_of_the_marker_without_a_v() {
        assert_eq!(version_of_marker("2026.10.26 yt-dlp.exe").as_deref(), Some("2026.10.26"));
        assert_eq!(version_of_marker("v2.7.7 deno-x86_64.zip").as_deref(), Some("2.7.7"));
        assert_eq!(version_of_marker("  "), None);
    }

    #[test]
    fn to_hex_is_lowercase_and_zero_padded() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xab]), "000fab");
    }

    #[test]
    fn backoff_waits_one_then_three_seconds() {
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(3));
    }

    fn fail(retry: bool) -> Failure {
        Failure { message: format!("retry={retry}"), retry }
    }

    #[tokio::test]
    async fn retryable_failures_are_tried_again_up_to_the_limit() {
        let calls = AtomicU32::new(0);
        let result: Result<(), String> = retrying(3, |_| Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(fail(true)) }
        })
        .await;
        assert_eq!(result, Err("retry=true".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_fatal_failure_is_not_retried() {
        let calls = AtomicU32::new(0);
        let result: Result<(), String> = retrying(3, |_| Duration::ZERO, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(fail(false)) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_later_success_ends_the_retries() {
        let calls = AtomicU32::new(0);
        let result = retrying(3, |_| Duration::ZERO, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(fail(true))
                } else {
                    Ok(n)
                }
            }
        })
        .await;
        assert_eq!(result, Ok(1));
    }

    #[test]
    fn download_urls_must_be_https_inside_the_release_folder() {
        let prefix = "https://github.com/yt-dlp/yt-dlp/releases/download/";
        assert!(check_url("https://github.com/yt-dlp/yt-dlp/releases/download/2026.10.26/yt-dlp.exe", prefix).is_ok());
        assert!(check_url("https://evil.example/yt-dlp.exe", prefix).is_err());
        assert!(check_url("http://github.com/yt-dlp/yt-dlp/releases/download/x/yt-dlp", prefix).is_err());
        assert!(check_url("https://github.com/yt-dlp/yt-dlp/releases/download/../../evil/x", prefix).is_err());
        assert!(check_url("https://github.com/x", "https://github.com").is_err(), "prefix must end in /");
        assert!(check_url(&format!("{prefix}%2e%2e/%2e%2e/%2e%2e/evil/repo/x"), prefix).is_err());
        assert!(check_url("https://github.com.evil.example/yt-dlp/yt-dlp/releases/download/x", prefix).is_err());
    }

    #[test]
    fn old_binary_name_keeps_its_extension() {
        assert_eq!(old_path(Path::new("dir/yt-dlp.exe")), Path::new("dir/yt-dlp.exe.old"));
        assert_eq!(old_path(Path::new("dir/deno")), Path::new("dir/deno.old"));
    }

    #[test]
    fn swap_in_installs_a_new_file_and_replaces_an_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("tool");
        let partial = dir.path().join("tool.download");
        fs::write(&partial, b"v1").unwrap();
        swap_in(&partial, &target).unwrap();
        fs::write(&partial, b"v2").unwrap();
        swap_in(&partial, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2");
        assert!(!partial.exists() && !old_path(&target).exists());
    }

    #[test]
    fn a_failed_swap_removes_the_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let partial = dir.path().join("tool.download");
        fs::write(&partial, b"x").unwrap();
        assert!(swap_in(&partial, &dir.path().join("missing-dir").join("tool")).is_err());
        assert!(!partial.exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_running_binary_is_moved_aside_instead_of_overwritten() {
        // An open handle without FILE_SHARE_DELETE stands in for the running program: it blocks overwriting.
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("tool.exe");
        let partial = dir.path().join("tool.download");
        fs::write(&target, b"old").unwrap();
        fs::write(&partial, b"new").unwrap();
        let held = fs::OpenOptions::new().read(true).share_mode(0x1 | 0x4).open(&target).unwrap(); // read | delete
        swap_in(&partial, &target).unwrap();
        drop(held);
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn leftovers_are_removed_and_binaries_kept() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["yt-dlp.exe", "yt-dlp.exe.old", "deno.download", "yt-dlp.version"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        remove_leftovers(dir.path());
        let mut left: Vec<String> =
            fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["yt-dlp.exe", "yt-dlp.version"]);
    }

    #[test]
    fn a_check_is_due_after_the_interval_or_when_never_done() {
        let day = Duration::from_secs(86_400);
        assert!(due(None, 1_000, day));
        assert!(!due(Some(1_000), 1_000 + 86_399, day));
        assert!(due(Some(1_000), 1_000 + 86_400, day));
        assert!(due(Some(5_000), 1_000, day), "a clock that went back must not stop checks");
    }

    #[test]
    fn stamp_round_trips_and_garbage_counts_as_never_checked() {
        let dir = tempfile::tempdir().unwrap();
        let stamp = dir.path().join("yt-dlp.checked");
        assert_eq!(last_checked(&stamp), None);
        mark_checked(&stamp);
        assert!(last_checked(&stamp).is_some_and(|t| t > 0));
        assert!(!check_due(Tool::YtDlp, &stamp));
        fs::write(&stamp, "garbage").unwrap();
        assert!(check_due(Tool::Deno, &stamp));
    }

    #[test]
    fn yt_dlp_is_checked_daily_and_the_others_weekly() {
        assert_eq!(Tool::YtDlp.check_interval(), Duration::from_secs(86_400));
        assert_eq!(Tool::Deno.check_interval(), Duration::from_secs(7 * 86_400));
        assert_eq!(Tool::Ffmpeg.check_interval(), Duration::from_secs(7 * 86_400));
    }

    #[test]
    fn site_errors_that_an_update_fixes_are_recognised() {
        assert!(looks_outdated("ERROR: [youtube] abc: nsig extraction failed: Some formats may be missing"));
        assert!(looks_outdated("ERROR: Unable to extract uploader id; please report this issue"));
        assert!(looks_outdated("Confirm you are on the latest version using  yt-dlp -U"));
        assert!(!looks_outdated("ERROR: [youtube] abc: Private video. Sign in if you've been granted access"));
        assert!(!looks_outdated("ERROR: unable to download video data: HTTP Error 404: Not Found"));
    }

    #[test]
    fn status_is_sent_to_the_ui_in_camel_case() {
        let json = serde_json::to_value(ToolsStatus::default()).unwrap();
        assert_eq!(json["ytDlp"]["phase"], "checking");
        assert_eq!(json["ffmpeg"]["usable"], false);
        assert_eq!(serde_json::to_value(Source::System).unwrap(), "system");
    }
}
