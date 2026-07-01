use crate::download::TrackDownload;
use lofty::file::TaggedFileExt;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::time::Instant;

use crate::utils::{sanitize, set_hidden};
use lofty::config::WriteOptions;
use lofty::picture::{Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, Tag, TagExt};
use tauri::{path::BaseDirectory, AppHandle, Manager};
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
) -> Result<String, String> {
    let app_data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    let batch_file_path = app_data.join("yt_batch_queue.txt");

    if !app_data.exists() {
        fs::create_dir_all(&app_data).map_err(|e| e.to_string())?;
    }

    // Write ONLY the raw URLs to the file—no arguments, no complex escaping
    let file_content: String = tracks
        .iter()
        .map(|t| t.url.as_str())
        .collect::<Vec<&str>>()
        .join("\n");

    fs::write(&batch_file_path, file_content).map_err(|e| e.to_string())?;

    let temp_stage_dir = format!("{}/.temp", output_dir);
    let stage_path = Path::new(&temp_stage_dir);

    if !stage_path.exists() {
        fs::create_dir_all(stage_path).map_err(|e| e.to_string())?;

        let _ = set_hidden(stage_path, true);
    }

    let output_template = format!("{temp_stage_dir}/%(id)s.%(ext)s");
    let batch_path_str = batch_file_path
        .to_str()
        .ok_or("Invalid temporary batch path encoding")?;

    #[rustfmt::skip]
    let base_args = vec![
        // "-f", "251/bestaudio",
        "-f", "ba[ext=m4a]/bestaudio[ext=m4a]",
        "--ignore-errors",
        "--format-sort", "hasaud,acodec,abr,channels,asr,aext",
        "--no-warnings",
        "--no-check-certificates",
        "--cookies", "Z:\\cookies.txt",
        "-a", batch_path_str,
        "-o", &output_template,
    ];

    println!(
        "[ytdlp::youtube] Streaming single-process batch download of size {} into temporary workspace...",
        tracks.len()
    );

    let mut result = execute(app, &base_args).await;

    if let Err(ref err) = result {
        let needs_auth = err.contains("Sign in to confirm your age");
        let dpapi_failed = err.contains("Failed to decrypt with DPAPI");
        let missing_db = err.contains("could not find") && err.contains("cookies database");

        if needs_auth || dpapi_failed || missing_db {
            println!("[ytdlp::youtube] Auth barrier or DPAPI error encountered. Cycling browser profiles...");

            let fallbacks = ["firefox", "chrome", "edge", "brave", "safari"];
            for browser in fallbacks {
                println!(
                    "[ytdlp::youtube] Attempting authentication fallback via profile: {}",
                    browser
                );
                let mut retry_args = base_args.clone();
                retry_args.push("--cookies-from-browser");
                retry_args.push(browser);

                let retry_result = execute(app, &retry_args).await;
                if retry_result.is_ok() {
                    result = retry_result;
                    break;
                } else if let Err(ref retry_err) = retry_result {
                    // If the specific browser failed due to DPAPI or age gating, keep cycling
                    let still_blocked = retry_err.contains("Sign in to confirm your age")
                        || retry_err.contains("Failed to decrypt with DPAPI");

                    if !still_blocked {
                        result = retry_result;
                        break;
                    }
                }
            }
        }
    }
    // Clean up tracking file asset from disk silently once done
    let _ = fs::remove_file(batch_file_path);

    result?;

    println!(
        "[Post-Processor] Processing {} downloaded files with Lofty...",
        tracks.len()
    );
    let base_output = Path::new(output_dir);
    let stage_path = Path::new(&temp_stage_dir);

    for track in tracks.iter() {
        let clean_track_name = sanitize(&track.title);

        // Extract the YouTube ID from the track URL (e.g., watch?v=0VuqQlORCLM -> 0VuqQlORCLM)
        let video_id = track.url.split("v=").nth(1).unwrap_or("");
        if video_id.is_empty() {
            eprintln!("[Error] Could not extract video ID from URL: {}", track.url);
            continue;
        }

        // Scan staging dir looking for a file matching that specific video ID
        let mut temp_file_path = None;
        if let Ok(entries) = fs::read_dir(stage_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if file_stem == video_id {
                        temp_file_path = Some(path);
                        break;
                    }
                }
            }
        }

        let temp_file_path = match temp_file_path {
            Some(path) => path,
            None => {
                eprintln!(
                    "[Warning] Staged target file missing for title: {}",
                    track.title
                );
                continue;
            }
        };

        // Keep track of whatever extension yt-dlp actually used (.webm, .m4a, etc.)
        let actual_ext = temp_file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3"); // fallback

        // 1. Inject Metadata with Lofty
        if let Err(err) = apply_metadata_tags(&temp_file_path, track).await {
            eprintln!(
                "[Metadata Error] Failed tag mapping for {}: {}",
                track.title, err
            );
        }

        // 2. Compute Target Directories
        let artist_folder = if !track.artists.is_empty() {
            sanitize(&track.artists[0])
        } else {
            "Unknown Artist".to_string()
        };

        let album_folder = if !track.album.is_empty() {
            sanitize(&track.album)
        } else {
            "Unknown Album".to_string()
        };

        let final_dest_dir = base_output.join(artist_folder).join(album_folder);
        if let Err(e) = fs::create_dir_all(&final_dest_dir) {
            return Err(format!("Could not create structural user folders: {}", e));
        }

        // Use the actual extension found during staging
        let final_file_name = match track.track_number {
            Some(num) => format!("{:02} - {}.{}", num, clean_track_name, actual_ext),
            None => format!("{}.{}", clean_track_name, actual_ext),
        };

        let destination_path = final_dest_dir.join(final_file_name);

        // Move target out of staging into our structured library tree
        fs::rename(&temp_file_path, &destination_path)
            .map_err(|e| format!("Failed shifting file from staging into library: {}", e))?;
    }
    // Clean up the empty `.temp` hidden layout folder
    let _ = fs::remove_dir(stage_path);

    Ok("Batch collection fully processed, tagged, and organized.".to_string())
}

async fn apply_metadata_tags(file_path: &Path, track: &TrackDownload) -> Result<(), String> {
    // Probe structural byte layouts to find primary containers (ID3v2 for MP3)
    let mut tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    // Grab or initialize the primary native audio tag
    let tag = match tagged_file.primary_tag_mut() {
        Some(existing_tag) => existing_tag,
        None => {
            let primary_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(primary_type));
            tagged_file.primary_tag_mut().unwrap()
        }
    };

    tag.set_title(track.title.clone());
    tag.set_artist(track.artists.join(", "));
    tag.set_album(track.album.clone());

    if let Some(year) = track.release_year {
        if let Ok(timestamp) = Timestamp::from_str(&year.to_string()) {
            tag.set_date(timestamp);
        }
    }
    if let Some(track_num) = track.track_number {
        tag.set_track(track_num as u32);
    }

    // Embed local cover art if it exists
    if let Some(parent_dir) = file_path.parent() {
        let local_cover = parent_dir.join("cover.jpg");
        if local_cover.exists() {
            if let Ok(mut img_file) = File::open(&local_cover) {
                if let Ok(picture) = Picture::from_reader(&mut img_file) {
                    let mut final_pic = picture;
                    final_pic.set_pic_type(PictureType::CoverFront);
                    tag.push_picture(final_pic);
                }
            }
        }
    }

    // Commit changes back to the filesystem binary stream
    tag.save_to_path(file_path, WriteOptions::default())
        .map_err(|e| e.to_string())?;
    Ok(())
}
