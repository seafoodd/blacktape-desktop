use crate::download::{ProgressEvent, TrackDownload};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Instant, SystemTime};

use crate::utils::set_hidden;
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

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

pub async fn download_track(
    app: &AppHandle,
    yt_url: &str,
    output_dir: &str,
    file_name: &str,
) -> Result<String, String> {
    let output_template = format!("{}/{}.%(ext)s", output_dir, file_name);

    #[rustfmt::skip]
    let args = [
        "-f", "bestaudio",
        "--format-sort", "hasaud,acodec,abr,channels,asr,aext",
        "--no-warnings",
        "--no-check-certificates",
        "--socket-timeout", "5",
        "-o", &output_template,
        yt_url,
    ];

    execute(app, &args).await
}

pub async fn download_batch(
    app: &AppHandle,
    tracks: &[TrackDownload],
    output_dir: &str,
    task_id: &str,
) -> Result<Vec<(PathBuf, TrackDownload)>, String> {
    let app_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;

    if !app_data.exists() {
        fs::create_dir_all(&app_data).map_err(|e| e.to_string())?;
    }
    let timestamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let session_id = format!("batch_{}", timestamp);

    let batch_file_path = app_data.join(format!("{}.txt", session_id));

    let file_content: String = tracks
        .iter()
        .map(|t| t.url.as_str())
        .collect::<Vec<&str>>()
        .join("\n");

    fs::write(&batch_file_path, file_content).map_err(|e| e.to_string())?;

    let temp_dir = format!("{output_dir}/.temp");
    let temp_batch_dir = format!("{output_dir}/.temp/{session_id}");
    let temp_path = Path::new(&temp_dir);
    let temp_batch_path = Path::new(&temp_batch_dir);

    if !temp_path.exists() {
        fs::create_dir_all(temp_path).map_err(|e| e.to_string())?;
        let _ = set_hidden(temp_path, true);
    }

    if !temp_batch_path.exists() {
        fs::create_dir_all(temp_batch_path).map_err(|e| e.to_string())?;
    }

    let output_template = format!("{temp_batch_dir}/%(autonumber)s.%(ext)s");
    let batch_path_str = batch_file_path
        .to_str()
        .ok_or("Invalid temporary batch path encoding")?;

    let app_data_dir = app.path().app_data_dir().unwrap();
    let edge_profile_path = app_data_dir
        .join("browser-profiles")
        .join("EBWebView")
        .join("Default");

    let cookie_arg = format!("edge:{}", edge_profile_path.to_string_lossy());

    #[rustfmt::skip]
    let base_args = vec![
        // "-f", "ba[ext=webm]/ba",
        "--audio-quality", "0",
        "--extract-audio",
        "--audio-format", "opus",
        "--ignore-errors",
        "--format-sort", "hasaud,acodec,abr,channels,asr,aext",
        "--no-warnings",
        "--no-check-certificates",
        "--cookies-from-browser", &cookie_arg,
        "--extractor-args", "youtube:player_client=music",
        "-a", batch_path_str,
        "-o", &output_template,
    ];

    println!(
        "[ytdlp] [{}] Streaming batch download of size {}...",
        session_id,
        tracks.len()
    );

    let binary_path = get_path(app);
    if !binary_path.exists() {
        return Err("yt-dlp binary is missing.".to_string());
    }

    let mut cmd = Command::new(binary_path);
    cmd.args(&base_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        const FLAGS: u32 = 0x08000000 | 0x00004000;
        cmd.creation_flags(FLAGS);
    }

    // Spawn the process instead of waiting for it
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    // Spawn a background task to read stderr so the process doesn't deadlock if it logs too much
    let stderr = child.stderr.take().ok_or("Failed to open stderr")?;
    let mut stderr_lines = tokio::io::BufReader::new(stderr).lines();
    let stderr_handle = tokio::spawn(async move {
        let mut err_msg = String::new();
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            err_msg.push_str(&line);
            err_msg.push('\n');
        }
        err_msg
    });

    let total_tracks = tracks.len();
    let mut last_count = 0;

    // Poll the directory while yt-dlp is running
    loop {
        let mut current_count = 0;

        // Count completed files (files that don't have the .part extension)
        if let Ok(mut entries) = tokio::fs::read_dir(temp_batch_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let is_part = path.extension().and_then(|s| s.to_str()) == Some("part");
                if !is_part {
                    current_count += 1;
                }
            }
        }

        // Emit progress event if a new song finished downloading
        if current_count > last_count {
            last_count = current_count;
            let _ = app.emit(
                "download-task-progress",
                ProgressEvent {
                    task_id: task_id.to_string(),
                    current: current_count,
                    total: total_tracks,
                    track_title: format!(
                        "Downloaded {} of {} songs to temp",
                        current_count, total_tracks
                    ),
                },
            );
        }

        // Check if yt-dlp has finished
        match child.try_wait() {
            Ok(Some(status)) => {
                let err_msg = stderr_handle.await.unwrap_or_default();

                let _ = fs::remove_file(&batch_file_path);

                if !status.success() {
                    if temp_batch_path.exists() {
                        let _ = fs::remove_dir_all(temp_batch_path);
                    }
                    return Err(err_msg.trim().to_string());
                }
                break; // Success!
            }
            Ok(None) => {
                // Still running, wait 500ms before checking again
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                let _ = stderr_handle.await;
                return Err(e.to_string());
            }
        }
    }

    // if let Err(err) = result {
    //     if temp_batch_path.exists() {
    //         let _ = fs::remove_dir_all(temp_batch_path);
    //     }
    //     return Err(err);
    // }

    let mut downloaded = Vec::new();

    for (index, track) in tracks.iter().enumerate() {
        let mut temp_file_path = None;

        if let Ok(entries) = fs::read_dir(temp_batch_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(file_num) = file_stem.parse::<usize>() {
                        if file_num == index + 1 {
                            temp_file_path = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(path) = temp_file_path {
            downloaded.push((path, track.clone()));
        } else {
            eprintln!(
                "[Warning] [{}] Staged file missing for: {}",
                session_id, track.title
            );
        }
    }

    Ok(downloaded)
}
