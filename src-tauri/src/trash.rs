//! In-app Trash: moves deleted songs into `<library_dir>/.blacktape-trash/` so they can
//! be restored. See docs/trash/PLAN.md and docs/trash/DECISIONS.md.
//!
//! Layout of the logic: pure helpers (paths, naming, selection rules) at the top, core
//! functions that take the DB mutex (testable without Tauri), and thin `#[command]`
//! wrappers at the bottom. The core locks the DB only for DB steps, never for file IO,
//! so a slow cross-drive copy does not block other commands.

use crate::audio::player::AudioPlayer;
use crate::db::db::{Database, TrashRow};
use crate::types::Song;
use crate::utils::set_hidden;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{command, AppHandle, Emitter, Manager, State};

/// Name of the hidden trash folder inside the library dir (the scanner must skip it).
pub const TRASH_DIR_NAME: &str = ".blacktape-trash";

/// Tauri event emitted (payload `()`) after every trash mutation.
pub const TRASH_CHANGED_EVENT: &str = "trash-changed";

/// `app_settings` key holding the library folder, written by `scan_music` (D11).
pub const LIBRARY_DIR_KEY: &str = "library_dir";

/// The app database behind the same tokio mutex that Tauri manages.
pub type DbLock = tokio::sync::Mutex<Database>;

const RETENTION_KEY: &str = "trash_retention_days";
const MAX_SIZE_KEY: &str = "trash_max_size_mb";
/// Stored value that means "no limit" (`None`), so it differs from "never set".
const NONE_VALUE: &str = "none";
const SECS_PER_DAY: i64 = 86_400;
const CLEAN_INTERVAL: Duration = Duration::from_secs(3600);
/// A song that was just stopped can stay locked for a moment on Windows (D9), so a
/// failed rename is retried this many times, this far apart.
const RENAME_ATTEMPTS: u32 = 5;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(100);

/// One trashed song as shown in the Trash page. Plain snake_case on the wire.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TrashItem {
    pub id: i64,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    /// Cover image path or URL of the song, if it had one.
    #[serde(default)]
    pub cover_url: Option<String>,
    pub original_path: String,
    pub size_bytes: u64,
    /// Unix seconds.
    pub trashed_at: i64,
}

/// `None` = never delete by age / no size limit.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TrashSettings {
    pub retention_days: Option<u32>,
    pub max_size_mb: Option<u64>,
}

impl Default for TrashSettings {
    /// D3: 30 days, 5 GB.
    fn default() -> Self {
        Self {
            retention_days: Some(30),
            max_size_mb: Some(5 * 1024),
        }
    }
}

/// Result of a bulk operation: how many items succeeded plus one message per failure.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct TrashOutcome {
    pub done: usize,
    pub failures: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Trash folder for the current library (D11). Without a known library folder the
/// trash lives in `<app data>/trash`.
pub fn trash_root_dir(library_dir: Option<&str>, app_data_dir: &Path) -> PathBuf {
    match library_dir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(dir) => Path::new(dir).join(TRASH_DIR_NAME),
        None => app_data_dir.join("trash"),
    }
}

/// `file_name` with `suffix` placed before the extension: ("a.mp3", " (2)") -> "a (2).mp3".
fn with_suffix(file_name: &str, suffix: &str) -> String {
    let path = Path::new(file_name);
    match (
        path.file_stem().and_then(|s| s.to_str()),
        path.extension().and_then(|e| e.to_str()),
    ) {
        (Some(stem), Some(ext)) => format!("{stem}{suffix}.{ext}"),
        (Some(stem), None) => format!("{stem}{suffix}"),
        _ => format!("{file_name}{suffix}"),
    }
}

/// Where a restored file goes: its original path, or `<name> (restored).<ext>` when
/// something already sits there, then `(restored 2)`, `(restored 3)`... (D7).
pub fn restore_target(original: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if !exists(original) {
        return original.to_path_buf();
    }
    let dir = original.parent().unwrap_or_else(|| Path::new(""));
    let name = original
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("track");
    let mut n: u32 = 1;
    loop {
        let suffix = if n == 1 {
            " (restored)".to_string()
        } else {
            format!(" (restored {n})")
        };
        let candidate = dir.join(with_suffix(name, &suffix));
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// `dir/name`, or `dir/name (2)`, `(3)`... when that name is taken.
fn unique_child_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let mut n: u32 = 2;
    loop {
        let candidate = dir.join(with_suffix(name, &format!(" ({n})")));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Parses one stored setting: unset -> default, "none" -> None, invalid -> default.
fn parse_setting<T: FromStr>(raw: Option<&str>, default: Option<T>) -> Option<T> {
    match raw {
        None => default,
        Some(NONE_VALUE) => None,
        Some(value) => value.trim().parse::<T>().ok().or(default),
    }
}

fn parse_settings(retention: Option<&str>, max_size: Option<&str>) -> TrashSettings {
    let defaults = TrashSettings::default();
    TrashSettings {
        retention_days: parse_setting(retention, defaults.retention_days),
        max_size_mb: parse_setting(max_size, defaults.max_size_mb),
    }
}

fn encode_setting<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| NONE_VALUE.to_string(), |v| v.to_string())
}

/// Builds the list entry. A row whose stored song data cannot be read still shows,
/// titled after its file name.
pub fn item_from_row(row: &TrashRow) -> TrashItem {
    let (title, artists, album, cover_url) = match serde_json::from_str::<Song>(&row.song_json)
    {
        Ok(song) => (song.title, song.artists, song.album, song.cover_url),
        Err(_) => {
            let title = Path::new(&row.original_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
            (title, Vec::new(), String::new(), None)
        }
    };
    TrashItem {
        id: row.id,
        title,
        artists,
        album,
        cover_url,
        original_path: row.original_path.clone(),
        size_bytes: row.size_bytes.max(0) as u64,
        trashed_at: row.trashed_at,
    }
}

/// Rows trashed strictly before `cutoff` (unix secs).
pub fn expired_rows(rows: &[TrashRow], cutoff: i64) -> Vec<TrashRow> {
    rows.iter()
        .filter(|r| r.trashed_at < cutoff)
        .cloned()
        .collect()
}

/// The oldest rows to delete so the total size is at most `limit_bytes`. `rows` must be
/// sorted oldest first.
pub fn rows_over_limit(rows: &[TrashRow], limit_bytes: u64) -> Vec<TrashRow> {
    let mut total: u64 = rows.iter().map(|r| r.size_bytes.max(0) as u64).sum();
    let mut victims = Vec::new();
    for row in rows {
        if total <= limit_bytes {
            break;
        }
        total = total.saturating_sub(row.size_bytes.max(0) as u64);
        victims.push(row.clone());
    }
    victims
}

/// Whether a result changed anything the other windows must reload: something was
/// done, or a failure came after a partial change (a stuck entry removed, rows purged).
pub fn should_notify(outcome: &TrashOutcome) -> bool {
    outcome.done > 0 || !outcome.failures.is_empty()
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Moves a file. A failed rename is retried (D9: the file may still be locked right
/// after playback stopped). If renaming never works, falls back to copy + remove
/// (for example across drives); the copy is discarded if the original cannot be removed.
pub async fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    if !src.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "source file is missing",
        ));
    }
    for attempt in 0..RENAME_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(RENAME_RETRY_DELAY).await;
        }
        match fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            // Retrying cannot help across drives; go straight to copy + remove.
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => break,
            Err(_) => {}
        }
    }
    fs::copy(src, dst)?;
    if let Err(e) = fs::remove_file(src) {
        let _ = fs::remove_file(dst);
        return Err(e);
    }
    Ok(())
}

/// Deletes a trashed file. A file that is already gone counts as deleted.
fn remove_trashed_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Removes `dir` only when it is empty (D16, same rule as `delete_song_files`).
fn remove_dir_if_empty(dir: &Path) {
    let _ = fs::remove_dir(dir);
}

// ---------------------------------------------------------------------------
// Core operations (take the DB mutex; lock only for DB steps)
// ---------------------------------------------------------------------------

/// Moves the songs' files into `root` and removes them from the library (D6). A song
/// whose file is already missing is only removed from the library. Songs that cannot
/// be moved keep their row and are reported in `failures`.
pub async fn trash_songs_in(
    db: &DbLock,
    root: &Path,
    ids: &[i64],
    now: i64,
) -> Result<TrashOutcome, String> {
    let mut outcome = TrashOutcome::default();
    if ids.is_empty() {
        return Ok(outcome);
    }
    let songs = db
        .lock()
        .await
        .get_songs_by_ids(ids)
        .await
        .map_err(|e| e.to_string())?;

    for song in songs {
        let Some(id) = song.id else { continue };
        if !Path::new(&song.path).exists() {
            let stored = db.lock().await.remove_song_rows(id).await;
            match stored {
                Ok(()) => outcome.done += 1,
                Err(e) => outcome.failures.push(format!("{}: {e}", song.path)),
            }
            continue;
        }
        match trash_one(db, root, &song, id, now).await {
            Ok(()) => outcome.done += 1,
            Err(e) => outcome.failures.push(format!("{}: {e}", song.path)),
        }
    }
    Ok(outcome)
}

async fn trash_one(
    db: &DbLock,
    root: &Path,
    song: &Song,
    id: i64,
    now: i64,
) -> Result<(), String> {
    let src = PathBuf::from(&song.path);
    let size = fs::metadata(&src).map_err(|e| e.to_string())?.len();
    let song_json = serde_json::to_string(song).map_err(|e| e.to_string())?;

    if !root.exists() {
        fs::create_dir_all(root).map_err(|e| format!("could not create trash folder: {e}"))?;
        let _ = set_hidden(root, true);
    }
    let file_name = src.file_name().and_then(|n| n.to_str()).unwrap_or("track");
    let dst = unique_child_path(root, &format!("{now}_{file_name}"));

    // File IO happens without the DB lock.
    move_file(&src, &dst)
        .await
        .map_err(|e| format!("could not move to trash: {e}"))?;

    let dst_str = dst.to_string_lossy().into_owned();
    let stored = db
        .lock()
        .await
        .move_song_to_trash(id, &song_json, &song.path, &dst_str, size as i64, now)
        .await;
    match stored {
        Ok(_) => {
            if let Some(parent) = src.parent() {
                remove_dir_if_empty(parent);
            }
            Ok(())
        }
        Err(e) => {
            // Put the file back so the song is not lost between the two steps.
            let _ = move_file(&dst, &src).await;
            Err(e.to_string())
        }
    }
}

/// Newest first.
pub async fn list_items(db: &DbLock) -> Result<Vec<TrashItem>, String> {
    let rows = db
        .lock()
        .await
        .list_trash_rows()
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().rev().map(item_from_row).collect())
}

/// Puts items back at their original path (D7) and re-adds their songs (D16).
pub async fn restore_items(db: &DbLock, ids: &[i64]) -> Result<TrashOutcome, String> {
    let mut outcome = TrashOutcome::default();
    for &id in ids {
        let row = db
            .lock()
            .await
            .get_trash_row(id)
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            outcome
                .failures
                .push(format!("Item {id} is no longer in the trash"));
            continue;
        };
        match restore_row(db, &row).await {
            Ok(()) => outcome.done += 1,
            Err(e) => outcome.failures.push(format!("{}: {e}", row.original_path)),
        }
    }
    Ok(outcome)
}

async fn restore_row(db: &DbLock, row: &TrashRow) -> Result<(), String> {
    let trash_path = PathBuf::from(&row.trash_path);
    if !trash_path.exists() {
        // Nothing left to restore: drop the entry so it cannot get stuck (D16).
        db.lock()
            .await
            .delete_trash_item(row.id)
            .await
            .map_err(|e| e.to_string())?;
        return Err("the file is missing from the trash; the entry was removed".into());
    }

    let mut song: Song = serde_json::from_str(&row.song_json)
        .map_err(|e| format!("stored song data is unreadable: {e}"))?;

    let target = restore_target(Path::new(&row.original_path), |p| p.exists());
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create folder: {e}"))?;
    }
    move_file(&trash_path, &target)
        .await
        .map_err(|e| format!("could not restore file: {e}"))?;

    song.path = target.to_string_lossy().into_owned();
    let stored = db
        .lock()
        .await
        .restore_trashed_song(&song, row.id)
        .await;
    if let Err(e) = stored {
        let _ = move_file(&target, &trash_path).await;
        return Err(e.to_string());
    }
    Ok(())
}

/// Permanently deletes the given items.
pub async fn delete_items(db: &DbLock, ids: &[i64]) -> Result<TrashOutcome, String> {
    let mut outcome = TrashOutcome::default();
    let mut rows = Vec::new();
    for &id in ids {
        let row = db
            .lock()
            .await
            .get_trash_row(id)
            .await
            .map_err(|e| e.to_string())?;
        match row {
            Some(row) => rows.push(row),
            None => outcome
                .failures
                .push(format!("Item {id} is no longer in the trash")),
        }
    }
    purge(db, &rows, &mut outcome).await;
    Ok(outcome)
}

/// Permanently deletes everything in the trash.
pub async fn empty_items(db: &DbLock) -> Result<TrashOutcome, String> {
    let rows = db
        .lock()
        .await
        .list_trash_rows()
        .await
        .map_err(|e| e.to_string())?;
    let mut outcome = TrashOutcome::default();
    purge(db, &rows, &mut outcome).await;
    Ok(outcome)
}

/// Deletes each item's file, then its row. An item whose file cannot be deleted keeps
/// its row and is reported. Rows already removed stay removed even if a later item
/// fails, which is why the caller still reports the change.
async fn purge(db: &DbLock, rows: &[TrashRow], outcome: &mut TrashOutcome) {
    for row in rows {
        match remove_trashed_file(Path::new(&row.trash_path)) {
            Ok(()) => {
                let stored = db.lock().await.delete_trash_item(row.id).await;
                match stored {
                    Ok(()) => outcome.done += 1,
                    Err(e) => outcome.failures.push(format!("{}: {e}", row.original_path)),
                }
            }
            Err(e) => outcome.failures.push(format!("{}: {e}", row.original_path)),
        }
    }
}

/// Automatic clean (D8): first drops items older than the retention, then drops the
/// oldest items until the trash fits the size limit.
pub async fn auto_clean(
    db: &DbLock,
    now: i64,
    settings: &TrashSettings,
) -> Result<TrashOutcome, String> {
    let mut outcome = TrashOutcome::default();

    if let Some(days) = settings.retention_days {
        let cutoff = now.saturating_sub(i64::from(days) * SECS_PER_DAY);
        let rows = db
            .lock()
            .await
            .list_trash_rows()
            .await
            .map_err(|e| e.to_string())?;
        purge(db, &expired_rows(&rows, cutoff), &mut outcome).await;
    }

    if let Some(max_mb) = settings.max_size_mb {
        let rows = db
            .lock()
            .await
            .list_trash_rows()
            .await
            .map_err(|e| e.to_string())?;
        let limit = max_mb.saturating_mul(1024 * 1024);
        purge(db, &rows_over_limit(&rows, limit), &mut outcome).await;
    }

    Ok(outcome)
}

/// Saves the settings, then applies them at once (lowering a limit takes effect now,
/// not at the next hourly pass).
pub async fn save_settings_and_clean(
    db: &DbLock,
    settings: &TrashSettings,
    now: i64,
) -> Result<TrashOutcome, String> {
    write_settings(&*db.lock().await, settings).await?;
    auto_clean(db, now, settings).await
}

pub async fn read_settings(db: &Database) -> Result<TrashSettings, String> {
    let retention = db
        .get_app_setting(RETENTION_KEY)
        .await
        .map_err(|e| e.to_string())?;
    let max_size = db
        .get_app_setting(MAX_SIZE_KEY)
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_settings(retention.as_deref(), max_size.as_deref()))
}

pub async fn write_settings(db: &Database, settings: &TrashSettings) -> Result<(), String> {
    db.set_app_setting(RETENTION_KEY, &encode_setting(settings.retention_days))
        .await
        .map_err(|e| e.to_string())?;
    db.set_app_setting(MAX_SIZE_KEY, &encode_setting(settings.max_size_mb))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri glue
// ---------------------------------------------------------------------------

/// Stops playback when the playing song is about to be trashed (D9). Queued copies are
/// left alone, like `delete_songs`.
fn stop_if_playing(player: &Mutex<AudioPlayer>, ids: &[i64]) {
    if let Ok(mut p) = player.lock() {
        if p.current_song_id().is_some_and(|id| ids.contains(&id)) {
            p.stop();
        }
    }
}

async fn trash_root(app: &AppHandle, db: &Database) -> Result<PathBuf, String> {
    let library_dir = db
        .get_app_setting(LIBRARY_DIR_KEY)
        .await
        .map_err(|e| e.to_string())?;
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(trash_root_dir(library_dir.as_deref(), &app_data))
}

fn notify(app: &AppHandle, outcome: &TrashOutcome) {
    if should_notify(outcome) {
        let _ = app.emit(TRASH_CHANGED_EVENT, ());
    }
}

/// Moves the songs' files to the trash and removes their DB rows (D6, D9).
#[command]
pub async fn trash_songs(
    app: AppHandle,
    db: State<'_, DbLock>,
    player: State<'_, Mutex<AudioPlayer>>,
    ids: Vec<i64>,
) -> Result<TrashOutcome, String> {
    stop_if_playing(&player, &ids);
    let root = {
        let guard = db.lock().await;
        trash_root(&app, &guard).await?
    };
    let outcome = trash_songs_in(&db, &root, &ids, now_secs()).await?;
    notify(&app, &outcome);
    Ok(outcome)
}

/// Newest first.
#[command]
pub async fn list_trash(db: State<'_, DbLock>) -> Result<Vec<TrashItem>, String> {
    list_items(&db).await
}

#[command]
pub async fn restore_trash(
    app: AppHandle,
    db: State<'_, DbLock>,
    ids: Vec<i64>,
) -> Result<TrashOutcome, String> {
    let outcome = restore_items(&db, &ids).await?;
    notify(&app, &outcome);
    Ok(outcome)
}

/// Permanent.
#[command]
pub async fn delete_trash(
    app: AppHandle,
    db: State<'_, DbLock>,
    ids: Vec<i64>,
) -> Result<TrashOutcome, String> {
    let outcome = delete_items(&db, &ids).await?;
    notify(&app, &outcome);
    Ok(outcome)
}

/// Permanent, everything.
#[command]
pub async fn empty_trash(app: AppHandle, db: State<'_, DbLock>) -> Result<TrashOutcome, String> {
    let outcome = empty_items(&db).await?;
    notify(&app, &outcome);
    Ok(outcome)
}

#[command]
pub async fn get_trash_settings(db: State<'_, DbLock>) -> Result<TrashSettings, String> {
    let guard = db.lock().await;
    read_settings(&guard).await
}

/// `app` is injected by Tauri (not a JS argument). Saving applies the new limits at once
/// and always tells the other windows, since the Trash page shows the policy.
#[command]
pub async fn set_trash_settings(
    app: AppHandle,
    db: State<'_, DbLock>,
    retention_days: Option<u32>,
    max_size_mb: Option<u64>,
) -> Result<(), String> {
    let settings = TrashSettings {
        retention_days,
        max_size_mb,
    };
    let result = save_settings_and_clean(&db, &settings, now_secs()).await;
    let _ = app.emit(TRASH_CHANGED_EVENT, ());
    result.map(|outcome| {
        for failure in &outcome.failures {
            eprintln!("[trash] clean after settings change could not delete: {failure}");
        }
    })
}

/// Called once from `setup()` after the Database is managed (D10). Ensures the trash
/// tables exist, runs one clean immediately, then every hour (D8).
pub fn spawn_auto_clean(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            run_auto_clean(&app).await;
            tokio::time::sleep(CLEAN_INTERVAL).await;
        }
    });
}

/// One clean pass. The DB lock is taken only for DB steps, never across the sleep.
async fn run_auto_clean(app: &AppHandle) {
    let state = app.state::<DbLock>();
    if let Err(e) = state.lock().await.ensure_trash_schema().await {
        eprintln!("[trash] could not create trash tables: {e}");
        return;
    }
    let settings = {
        let guard = state.lock().await;
        match read_settings(&guard).await {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!("[trash] auto-clean failed: {e}");
                return;
            }
        }
    };

    match auto_clean(&state, now_secs(), &settings).await {
        Ok(outcome) => {
            for failure in &outcome.failures {
                eprintln!("[trash] auto-clean could not delete: {failure}");
            }
            notify(app, &outcome);
        }
        Err(e) => eprintln!("[trash] auto-clean failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::db::tests::setup_test_db;
    use crate::types::{Platform, QualityTier};

    const NOW: i64 = 1_700_000_000;
    const DAY: i64 = SECS_PER_DAY;
    const MB: i64 = 1024 * 1024;

    async fn test_db() -> DbLock {
        let db = setup_test_db().await;
        db.ensure_trash_schema().await.unwrap();
        DbLock::new(db)
    }

    fn s(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    fn song(path: &Path, title: &str) -> Song {
        Song {
            id: None,
            path: s(path),
            title: title.into(),
            artists: vec!["Artist".into()],
            album_artist: "Artist".into(),
            album: "Album".into(),
            duration_ms: 1000,
            track_number: Some(1),
            genre: None,
            release_year: Some(2020),
            cover_url: None,
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source: Platform::Local,
            source_url: None,
            source_item_id: None,
            canonical_track_slug: "artist-song".into(),
            canonical_album_slug: "artist-album".into(),
            quality_tier: QualityTier::Low,
        }
    }

    async fn song_id_by_path(db: &DbLock, path: &Path) -> i64 {
        let guard = db.lock().await;
        sqlx::query_scalar("SELECT id FROM songs WHERE path = ?")
            .bind(s(path))
            .fetch_one(&guard.pool)
            .await
            .unwrap()
    }

    /// Writes an 11-byte audio file and inserts its song row. Returns the song id.
    async fn add_song(db: &DbLock, path: &Path, title: &str) -> i64 {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"audio-bytes").unwrap();
        db.lock().await.insert_song(song(path, title)).await.unwrap();
        song_id_by_path(db, path).await
    }

    async fn songs_by_id(db: &DbLock, id: i64) -> Vec<Song> {
        db.lock().await.get_songs_by_ids(&[id]).await.unwrap()
    }

    async fn trash_list(db: &DbLock) -> Vec<TrashItem> {
        list_items(db).await.unwrap()
    }

    async fn playlist_songs(db: &DbLock, playlist: i64) -> Vec<Song> {
        db.lock().await.get_playlist_songs(playlist).await.unwrap()
    }

    /// Puts an item straight into the trash (file + row) with a chosen age and size.
    async fn add_trash_item(
        db: &DbLock,
        root: &Path,
        name: &str,
        size_bytes: i64,
        trashed_at: i64,
    ) -> (i64, PathBuf) {
        fs::create_dir_all(root).unwrap();
        let file = root.join(name);
        fs::write(&file, b"trashed").unwrap();
        let original = root.join("library").join(name);
        let json = serde_json::to_string(&song(&original, name)).unwrap();
        let guard = db.lock().await;
        let id = sqlx::query(
            "INSERT INTO trash_items (song_json, original_path, trash_path, size_bytes, trashed_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(json)
        .bind(s(&original))
        .bind(s(&file))
        .bind(size_bytes)
        .bind(trashed_at)
        .execute(&guard.pool)
        .await
        .unwrap()
        .last_insert_rowid();
        (id, file)
    }

    fn row(id: i64, original: &str, size_bytes: i64, trashed_at: i64) -> TrashRow {
        TrashRow {
            id,
            song_json: String::new(),
            original_path: original.into(),
            trash_path: format!("{original}.trashed"),
            size_bytes,
            trashed_at,
        }
    }

    // --- pure helpers -----------------------------------------------------

    #[test]
    fn trash_root_uses_library_dir_or_app_data_fallback() {
        let app = Path::new("/appdata");
        assert_eq!(
            trash_root_dir(Some("/music"), app),
            Path::new("/music").join(TRASH_DIR_NAME)
        );
        assert_eq!(trash_root_dir(None, app), app.join("trash"));
        assert_eq!(trash_root_dir(Some("  "), app), app.join("trash"));
    }

    #[test]
    fn restore_target_keeps_free_path() {
        let original = Path::new("/music/A/a.mp3");
        assert_eq!(restore_target(original, |_| false), original);
    }

    #[test]
    fn restore_target_adds_restored_suffix_on_collision() {
        let original = PathBuf::from("/music/A/a.mp3");
        let taken = original.clone();
        let target = restore_target(&original, |p| p == taken);
        assert_eq!(target, Path::new("/music/A/a (restored).mp3"));
    }

    #[test]
    fn restore_target_numbers_repeated_collisions() {
        let original = PathBuf::from("/music/a.mp3");
        let taken = vec![
            PathBuf::from("/music/a.mp3"),
            PathBuf::from("/music/a (restored).mp3"),
        ];
        let target = restore_target(&original, |p| taken.iter().any(|t| t == p));
        assert_eq!(target, Path::new("/music/a (restored 2).mp3"));
    }

    #[test]
    fn restore_target_handles_names_without_extension() {
        let original = PathBuf::from("/music/README");
        let taken = original.clone();
        let target = restore_target(&original, |p| p == taken);
        assert_eq!(target, Path::new("/music/README (restored)"));
    }

    #[test]
    fn unique_child_path_numbers_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_child_path(dir.path(), "x.mp3");
        assert_eq!(first, dir.path().join("x.mp3"));
        fs::write(&first, b"1").unwrap();
        let second = unique_child_path(dir.path(), "x.mp3");
        assert_eq!(second, dir.path().join("x (2).mp3"));
    }

    #[test]
    fn parse_settings_defaults_none_and_invalid_values() {
        assert_eq!(parse_settings(None, None), TrashSettings::default());
        assert_eq!(
            parse_settings(Some("none"), Some("none")),
            TrashSettings {
                retention_days: None,
                max_size_mb: None
            }
        );
        // Invalid stored values fall back to the default for that key only.
        assert_eq!(
            parse_settings(Some("soon"), Some("12")),
            TrashSettings {
                retention_days: Some(30),
                max_size_mb: Some(12)
            }
        );
    }

    #[test]
    fn expired_rows_are_strictly_older_than_cutoff() {
        let rows = vec![
            row(1, "/a", 1, NOW - 31 * DAY),
            row(2, "/b", 1, NOW - 30 * DAY),
            row(3, "/c", 1, NOW),
        ];
        let ids: Vec<i64> = expired_rows(&rows, NOW - 30 * DAY)
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn rows_over_limit_evicts_oldest_first_until_under_limit() {
        // Oldest first: three items of 600 MB, limit 1000 MB.
        let rows = vec![
            row(1, "/a", 600 * MB, NOW - 300),
            row(2, "/b", 600 * MB, NOW - 200),
            row(3, "/c", 600 * MB, NOW - 100),
        ];
        let ids: Vec<i64> = rows_over_limit(&rows, 1000 * MB as u64)
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec![1, 2]);
        assert!(rows_over_limit(&rows, 2000 * MB as u64).is_empty());
    }

    #[test]
    fn should_notify_also_covers_partial_changes() {
        assert!(should_notify(&TrashOutcome {
            done: 1,
            failures: vec![]
        }));
        // A failed restore that removed a stuck entry, or a purge that stopped early.
        assert!(should_notify(&TrashOutcome {
            done: 0,
            failures: vec!["x".into()]
        }));
        assert!(!should_notify(&TrashOutcome::default()));
    }

    #[test]
    fn item_from_row_falls_back_when_song_data_is_corrupt() {
        let r = TrashRow {
            id: 9,
            song_json: "{not json".into(),
            original_path: "/music/Artist/Track.mp3".into(),
            trash_path: "/music/.blacktape-trash/x".into(),
            size_bytes: 42,
            trashed_at: NOW,
        };
        let item = item_from_row(&r);
        assert_eq!(item.title, "Track");
        assert!(item.artists.is_empty());
        assert_eq!(item.size_bytes, 42);
        assert_eq!(item.cover_url, None);
    }

    #[test]
    fn item_from_row_carries_song_cover() {
        let song = crate::types::Song {
            id: Some(1),
            path: "/music/a.mp3".into(),
            title: "T".into(),
            artists: vec!["A".into()],
            album_artist: "A".into(),
            album: "Al".into(),
            duration_ms: 0,
            track_number: None,
            genre: None,
            release_year: None,
            cover_url: Some("C:/covers/x.jpg".into()),
            external_cover_url: None,
            lyrics: None,
            lyrics_source: None,
            source: crate::types::Platform::Local,
            source_url: None,
            source_item_id: None,
            canonical_track_slug: "t".into(),
            canonical_album_slug: "al".into(),
            quality_tier: crate::types::QualityTier::Low,
        };
        let r = TrashRow {
            id: 3,
            song_json: serde_json::to_string(&song).unwrap(),
            original_path: "/music/a.mp3".into(),
            trash_path: "/music/.blacktape-trash/x".into(),
            size_bytes: 1,
            trashed_at: NOW,
        };
        assert_eq!(item_from_row(&r).cover_url.as_deref(), Some("C:/covers/x.jpg"));
    }

    #[tokio::test]
    async fn move_file_moves_content_and_reports_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.mp3");
        let dst = dir.path().join("dst.mp3");
        fs::write(&src, b"abc").unwrap();

        move_file(&src, &dst).await.unwrap();

        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"abc");
        let err = move_file(&src, &dir.path().join("other.mp3"))
            .await
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    // --- core operations (in-memory DB + temp folders) --------------------

    #[tokio::test]
    async fn trash_and_restore_roundtrip() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let album = lib.path().join("Artist").join("Album");
        let file = album.join("01 song.mp3");
        let id = add_song(&db, &file, "Song").await;
        let playlist = db.lock().await.create_playlist("Mix", None, None).await.unwrap();
        db.lock()
            .await
            .add_songs_to_playlist(playlist, &[id])
            .await
            .unwrap();

        let outcome = trash_songs_in(&db, &root, &[id], NOW).await.unwrap();
        assert_eq!(
            outcome,
            TrashOutcome {
                done: 1,
                failures: vec![]
            }
        );
        assert!(!file.exists());
        assert!(!album.exists(), "emptied album folder is removed");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(songs_by_id(&db, id).await.is_empty());
        assert!(playlist_songs(&db, playlist).await.is_empty());

        let items = trash_list(&db).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Song");
        assert_eq!(items[0].artists, vec!["Artist".to_string()]);
        assert_eq!(items[0].album, "Album");
        assert_eq!(items[0].size_bytes, 11);
        assert_eq!(items[0].trashed_at, NOW);
        assert_eq!(items[0].original_path, s(&file));

        let restored = restore_items(&db, &[items[0].id]).await.unwrap();
        assert_eq!(
            restored,
            TrashOutcome {
                done: 1,
                failures: vec![]
            }
        );
        assert_eq!(fs::read(&file).unwrap(), b"audio-bytes");
        let songs = songs_by_id(&db, id).await;
        assert_eq!(songs.len(), 1, "same id is kept when it is free");
        assert_eq!(songs[0].path, s(&file));
        assert!(trash_list(&db).await.is_empty());
        assert!(
            playlist_songs(&db, playlist).await.is_empty(),
            "playlist membership is not restored (D6)"
        );
    }

    #[tokio::test]
    async fn restore_uses_restored_name_when_original_path_is_taken() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let file = lib.path().join("Artist").join("01 song.mp3");
        let id = add_song(&db, &file, "Song").await;
        trash_songs_in(&db, &root, &[id], NOW).await.unwrap();
        let item = trash_list(&db).await.remove(0);

        // Something new now sits at the original path (its folder was removed when trashed).
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"new file").unwrap();

        let outcome = restore_items(&db, &[item.id]).await.unwrap();
        assert_eq!(outcome.done, 1);
        assert!(outcome.failures.is_empty());
        assert_eq!(fs::read(&file).unwrap(), b"new file");

        let restored = file.with_file_name("01 song (restored).mp3");
        assert_eq!(fs::read(&restored).unwrap(), b"audio-bytes");
        let songs = songs_by_id(&db, id).await;
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].path, s(&restored));
    }

    #[tokio::test]
    async fn restore_gets_new_id_when_old_id_is_taken() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let file = lib.path().join("a.mp3");
        let id = add_song(&db, &file, "Song").await;
        trash_songs_in(&db, &root, &[id], NOW).await.unwrap();
        let item = trash_list(&db).await.remove(0);

        // Another song now owns the old id.
        sqlx::query(
            "INSERT INTO songs (id, path, title, duration_ms) VALUES (?, '/other.mp3', 'Other', 1)",
        )
        .bind(id)
        .execute(&db.lock().await.pool)
        .await
        .unwrap();

        let outcome = restore_items(&db, &[item.id]).await.unwrap();
        assert_eq!(outcome.done, 1);
        let new_id = song_id_by_path(&db, &file).await;
        assert_ne!(new_id, id);
        let other: String = sqlx::query_scalar("SELECT title FROM songs WHERE id = ?")
            .bind(id)
            .fetch_one(&db.lock().await.pool)
            .await
            .unwrap();
        assert_eq!(other, "Other");
    }

    #[tokio::test]
    async fn restore_drops_entry_and_reports_when_trash_file_is_missing() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let file = lib.path().join("a.mp3");
        let id = add_song(&db, &file, "Song").await;
        trash_songs_in(&db, &root, &[id], NOW).await.unwrap();
        let item = trash_list(&db).await.remove(0);
        let trash_file = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
        fs::remove_file(&trash_file).unwrap();

        let outcome = restore_items(&db, &[item.id]).await.unwrap();

        assert_eq!(outcome.done, 0);
        assert_eq!(outcome.failures.len(), 1);
        assert!(should_notify(&outcome), "the stuck entry was removed, so other windows must reload");
        assert!(trash_list(&db).await.is_empty());
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn trash_drops_library_entry_for_missing_file_without_trash_item() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let gone = lib.path().join("gone.mp3");
        db.lock().await.insert_song(song(&gone, "Gone")).await.unwrap();
        let id = song_id_by_path(&db, &gone).await;

        let outcome = trash_songs_in(&db, &root, &[id], NOW).await.unwrap();

        assert_eq!(outcome.done, 1);
        assert!(outcome.failures.is_empty());
        assert!(songs_by_id(&db, id).await.is_empty());
        assert!(trash_list(&db).await.is_empty());
    }

    #[tokio::test]
    async fn trash_keeps_song_when_it_cannot_be_moved() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let file = lib.path().join("a.mp3");
        let id = add_song(&db, &file, "Song").await;
        // The trash "folder" is a plain file, so nothing can be moved into it.
        let blocker = lib.path().join("blocker");
        fs::write(&blocker, b"x").unwrap();

        let outcome = trash_songs_in(&db, &blocker, &[id], NOW).await.unwrap();

        assert_eq!(outcome.done, 0);
        assert_eq!(outcome.failures.len(), 1);
        assert!(file.exists());
        assert_eq!(songs_by_id(&db, id).await.len(), 1);
        assert!(trash_list(&db).await.is_empty());
    }

    #[tokio::test]
    async fn trashing_closes_gap_in_playlist_positions() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let a = add_song(&db, &lib.path().join("a.mp3"), "A").await;
        let b = add_song(&db, &lib.path().join("b.mp3"), "B").await;
        let c = add_song(&db, &lib.path().join("c.mp3"), "C").await;
        let playlist = db.lock().await.create_playlist("Mix", None, None).await.unwrap();
        db.lock()
            .await
            .add_songs_to_playlist(playlist, &[a, b, c])
            .await
            .unwrap();

        trash_songs_in(&db, &root, &[b], NOW).await.unwrap();

        let titles: Vec<String> = playlist_songs(&db, playlist)
            .await
            .into_iter()
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, vec!["A", "C"]);
        let positions: Vec<i64> = sqlx::query_scalar(
            "SELECT position FROM playlist_songs WHERE playlist_id = ? ORDER BY position",
        )
        .bind(playlist)
        .fetch_all(&db.lock().await.pool)
        .await
        .unwrap();
        assert_eq!(positions, vec![1, 2]);
    }

    #[tokio::test]
    async fn list_is_newest_first() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        add_trash_item(&db, &root, "old.mp3", 1, NOW - 300).await;
        add_trash_item(&db, &root, "new.mp3", 1, NOW - 10).await;
        add_trash_item(&db, &root, "mid.mp3", 1, NOW - 100).await;

        let titles: Vec<String> = trash_list(&db).await.into_iter().map(|i| i.title).collect();
        assert_eq!(titles, vec!["new.mp3", "mid.mp3", "old.mp3"]);
    }

    #[tokio::test]
    async fn delete_and_empty_remove_files_and_rows() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        let first = add_song(&db, &lib.path().join("a.mp3"), "A").await;
        let second = add_song(&db, &lib.path().join("b.mp3"), "B").await;
        trash_songs_in(&db, &root, &[first, second], NOW)
            .await
            .unwrap();
        let items = trash_list(&db).await;
        let newer = items.iter().find(|i| i.title == "B").unwrap().id;

        let deleted = delete_items(&db, &[newer]).await.unwrap();
        assert_eq!(deleted.done, 1);
        assert_eq!(trash_list(&db).await.len(), 1);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        let emptied = empty_items(&db).await.unwrap();
        assert_eq!(emptied.done, 1);
        assert!(trash_list(&db).await.is_empty());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn delete_keeps_row_when_file_cannot_be_removed() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        // A directory where the trashed file should be: remove_file cannot delete it.
        let stuck = root.join("stuck");
        fs::create_dir_all(&stuck).unwrap();
        let json = serde_json::to_string(&song(&stuck, "Stuck")).unwrap();
        let id = sqlx::query(
            "INSERT INTO trash_items (song_json, original_path, trash_path, size_bytes, trashed_at)
             VALUES (?, '/x.mp3', ?, 1, ?)",
        )
        .bind(json)
        .bind(s(&stuck))
        .bind(NOW)
        .execute(&db.lock().await.pool)
        .await
        .unwrap()
        .last_insert_rowid();

        let outcome = delete_items(&db, &[id]).await.unwrap();

        assert_eq!(outcome.done, 0);
        assert_eq!(outcome.failures.len(), 1);
        assert_eq!(trash_list(&db).await.len(), 1);
    }

    #[tokio::test]
    async fn retention_cleanup_removes_only_expired_items() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        add_trash_item(&db, &root, "old.mp3", 1, NOW - 31 * DAY).await;
        let (_, recent_file) = add_trash_item(&db, &root, "recent.mp3", 1, NOW - DAY).await;
        let settings = TrashSettings {
            retention_days: Some(30),
            max_size_mb: None,
        };

        let outcome = auto_clean(&db, NOW, &settings).await.unwrap();

        assert_eq!(outcome.done, 1);
        assert!(outcome.failures.is_empty());
        assert!(recent_file.exists());
        assert!(!root.join("old.mp3").exists());
        let remaining = trash_list(&db).await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "recent.mp3");
    }

    #[tokio::test]
    async fn size_cleanup_removes_oldest_first() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        add_trash_item(&db, &root, "t1.mp3", 600 * MB, NOW - 300).await;
        add_trash_item(&db, &root, "t2.mp3", 600 * MB, NOW - 200).await;
        add_trash_item(&db, &root, "t3.mp3", 600 * MB, NOW - 100).await;
        let settings = TrashSettings {
            retention_days: None,
            max_size_mb: Some(1000),
        };

        let outcome = auto_clean(&db, NOW, &settings).await.unwrap();

        assert_eq!(outcome.done, 2);
        assert!(!root.join("t1.mp3").exists());
        assert!(!root.join("t2.mp3").exists());
        assert!(root.join("t3.mp3").exists());
        let remaining = trash_list(&db).await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "t3.mp3");
    }

    #[tokio::test]
    async fn saving_lower_limit_cleans_immediately() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        add_trash_item(&db, &root, "t1.mp3", 600 * MB, NOW - 300).await;
        add_trash_item(&db, &root, "t2.mp3", 600 * MB, NOW - 100).await;
        // Default limit (5 GB) would keep both; the new 1 GB limit must apply at once.
        let settings = TrashSettings {
            retention_days: Some(30),
            max_size_mb: Some(1000),
        };

        let outcome = save_settings_and_clean(&db, &settings, NOW).await.unwrap();

        assert_eq!(outcome.done, 1);
        assert_eq!(read_settings(&*db.lock().await).await.unwrap(), settings);
        let remaining = trash_list(&db).await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].title, "t2.mp3");
    }

    #[tokio::test]
    async fn auto_clean_without_limits_keeps_everything() {
        let db = test_db().await;
        let lib = tempfile::tempdir().unwrap();
        let root = lib.path().join(TRASH_DIR_NAME);
        add_trash_item(&db, &root, "ancient.mp3", 900 * MB, NOW - 5000 * DAY).await;
        let settings = TrashSettings {
            retention_days: None,
            max_size_mb: None,
        };

        let outcome = auto_clean(&db, NOW, &settings).await.unwrap();

        assert_eq!(outcome.done, 0);
        assert_eq!(trash_list(&db).await.len(), 1);
    }

    #[tokio::test]
    async fn settings_default_then_round_trip() {
        let db = test_db().await;
        assert_eq!(
            read_settings(&*db.lock().await).await.unwrap(),
            TrashSettings::default()
        );

        let custom = TrashSettings {
            retention_days: Some(7),
            max_size_mb: Some(2048),
        };
        write_settings(&*db.lock().await, &custom).await.unwrap();
        assert_eq!(read_settings(&*db.lock().await).await.unwrap(), custom);

        let unlimited = TrashSettings {
            retention_days: None,
            max_size_mb: None,
        };
        write_settings(&*db.lock().await, &unlimited).await.unwrap();
        assert_eq!(read_settings(&*db.lock().await).await.unwrap(), unlimited);
    }

    #[tokio::test]
    async fn app_settings_upsert_keeps_latest_value() {
        let db = test_db().await;
        let guard = db.lock().await;
        assert_eq!(guard.get_app_setting(LIBRARY_DIR_KEY).await.unwrap(), None);
        guard.set_app_setting(LIBRARY_DIR_KEY, "/music").await.unwrap();
        guard.set_app_setting(LIBRARY_DIR_KEY, "/other").await.unwrap();
        assert_eq!(
            guard.get_app_setting(LIBRARY_DIR_KEY).await.unwrap().as_deref(),
            Some("/other")
        );
    }
}
