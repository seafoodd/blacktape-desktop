use crate::download::{ProgressEvent, TrackDownload};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};
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

    let latest_version = response.tag_name;
    let binary_path = get_path(&app);
    let version_file = binary_path.with_extension("version");
    let local_version = fs::read_to_string(&version_file).unwrap_or_default();

    if !binary_path.exists() || local_version.trim() != latest_version.trim() {
        println!("[ytdlp] Upgrading binary to {}...", latest_version);

        #[cfg(target_os = "windows")]
        let target_asset = "yt-dlp.exe";
        #[cfg(target_os = "macos")]
        let target_asset = "yt-dlp_macos";
        #[cfg(target_os = "linux")]
        let target_asset = "yt-dlp";

        let asset = response
            .assets
            .iter()
            .find(|a| a.name == target_asset)
            .ok_or_else(|| "Binary asset not found in GitHub response".to_string())?;

        let mut download_resp = client
            .get(&asset.browser_download_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if let Some(parent) = binary_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let mut file = File::create(&binary_path).map_err(|e| e.to_string())?;
        while let Some(chunk) = download_resp.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&binary_path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&binary_path, perms).map_err(|e| e.to_string())?;
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
        Err(err_msg)
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
    binary_path: PathBuf,
    cookie_arg: String,
    temp_dir: PathBuf,
    session_id: String,
}

impl DownloadContext {
    async fn prepare(
        app: &AppHandle,
        output_dir: &str,
        session_prefix: &str,
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

        let temp_dir = PathBuf::from(output_dir).join(".temp").join(&session_id);
        tokio::fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| e.to_string())?;

        let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let edge_profile_path = app_data_dir
            .join("browser-profiles")
            .join("EBWebView")
            .join("Default");
        let cookie_arg = format!("edge:{}", edge_profile_path.to_string_lossy());

        Ok(Self {
            binary_path,
            cookie_arg,
            temp_dir,
            session_id,
        })
    }
}

async fn download_track_internal(
    ctx: &DownloadContext,
    track: &TrackDownload,
    track_num: usize,
    total_tracks: usize,
) -> Option<PathBuf> {
    let output_template = ctx.temp_dir.join(format!("{}.%(ext)s", track_num));
    let is_youtube = track.url.contains("youtube.com") || track.url.contains("youtu.be");

    let mut args = vec![
        "--extract-audio".to_string(),
        "--audio-format".to_string(),
        "best".to_string(),
        "--ignore-errors".to_string(),
        "--no-check-certificates".to_string(),
        "--no-playlist".to_string(),
        "--concurrent-fragments".to_string(),
        "10".to_string(),
        "--http-chunk-size".to_string(),
        "10M".to_string(),
        "-o".to_string(),
        output_template.to_string_lossy().to_string(),
    ];

    if is_youtube {
        args.extend(vec![
            "-f".to_string(),
            "ba[ext=webm]/ba[ext=m4a]/ba/ba*/bestaudio/b".to_string(),
            "--extractor-args".to_string(),
            "youtube:player_client=android,web".to_string(),
            "--cookies-from-browser".to_string(),
            ctx.cookie_arg.clone(),
        ]);
    } else {
        args.extend(vec!["-f".to_string(), "bestaudio/best".to_string()]);
    }

    args.push(track.url.clone());

    println!(
        "[yt-dlp] [{}] [Track {}/{}] Executing:\n  {:?} {}",
        ctx.session_id,
        track_num,
        total_tracks,
        ctx.binary_path,
        args.join(" ")
    );

    let mut cmd = Command::new(&ctx.binary_path);
    cmd.args(&args);

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = match cmd.output().await {
        Ok(out) => out,
        Err(err) => {
            eprintln!(
                "[yt-dlp ERROR] [{}] [Track {}/{}] Process failed to execute: {}",
                ctx.session_id, track_num, total_tracks, err
            );
            return None;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!(
            "[yt-dlp ERROR] [{}] [Track {}/{}] Exited with code {:?}\n--- STDERR ---\n{}\n--- STDOUT ---\n{}",
            ctx.session_id,
            track_num,
            total_tracks,
            output.status.code(),
            stderr.trim(),
            stdout.trim()
        );
        return None;
    }

    if !stderr.trim().is_empty() {
        println!(
            "[yt-dlp LOG] [{}] [Track {}/{}] Warnings/Info:\n{}",
            ctx.session_id,
            track_num,
            total_tracks,
            stderr.trim()
        );
    }

    find_downloaded_file(&ctx.temp_dir, track_num).await
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
    let ctx = Arc::new(DownloadContext::prepare(app, output_dir, "batch").await?);

    println!(
        "[yt-dlp] [{}] Starting batch of {} tracks. Output temp dir: {:?}",
        ctx.session_id,
        tracks.len(),
        ctx.temp_dir
    );

    let total_tracks = tracks.len();
    let completed_counter = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

    let mut tasks = Vec::new();

    for (index, track) in tracks.iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        let completed = Arc::clone(&completed_counter);
        let track = track.clone();
        let app_handle = app.clone();
        let ctx = Arc::clone(&ctx);
        let task_id = task_id.to_string();

        tasks.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

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
                        task_id,
                        current,
                        total: total_tracks,
                        track_title: format!("Downloaded {} of {} tracks", current, total_tracks),
                    },
                );

                Some((path.clone(), track))
            } else {
                eprintln!(
                    "[yt-dlp ERROR] [{}] [Track {}/{}] Process succeeded, but no output file matching '{}.*' was found in {:?}",
                    ctx.session_id, track_num, total_tracks, track_num, ctx.temp_dir
                );
                None
            }
        }));
    }

    let results = futures::future::join_all(tasks).await;
    let downloaded: Vec<(PathBuf, TrackDownload)> = results
        .into_iter()
        .filter_map(|r| {
            r.unwrap_or_else(|join_err| {
                eprintln!("[yt-dlp ERROR] Task panic/join error: {}", join_err);
                None
            })
        })
        .collect();

    println!(
        "[yt-dlp SUMMARY] Batch finished. Downloaded {}/{} tracks in {:?}",
        downloaded.len(),
        total_tracks,
        now.elapsed()
    );

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
