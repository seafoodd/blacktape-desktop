use crate::download::{ytdlp, AlbumDownload, TrackDownload};
use crate::utils::sanitize;
use rustypipe::client::RustyPipe;
use tauri::AppHandle;

/// Extracts streaming URL of a single YouTube track
pub async fn extract_streaming_url(app: &AppHandle, yt_url: &str) -> Result<String, String> {
    let args = [
        "--no-playlist",
        "-g",
        "--format-sort",
        "hasaud,acodec,abr,channels,asr,aext",
        "-f",
        "bestaudio",
        "--ignore-errors",
        "--no-warnings",
        "--cookies",
        "C:\\Users\\seafood\\Downloads\\youtube.txt",
        yt_url,
    ];

    ytdlp::execute(app, &args).await
}

/// Parses a YouTube Music `browse_url` or album page and returns an [`AlbumDownload`].
///
/// **Note:** Direct YouTube playlist URLs (e.g., `playlist?list=...`) are not supported.
///
/// # Arguments
/// * `browse_url` - A valid browse URL or unique browse ID from YouTube Music (e.g., `"MPREb_QGz4un0op6F"`).
///
/// # Errors
/// Returns a `String` error if the URL format is invalid, a playlist is provided,
/// or the inner network request fails to fetch the album content.
pub async fn parse_album(browse_url: &str) -> Result<AlbumDownload, String> {
    let browse_id = extract_browse_id(browse_url)?;

    let rp = RustyPipe::new();
    let album = rp
        .query()
        .music_album(browse_id)
        .await
        .map_err(|e| format!("RustyPipe failed to fetch album: {}", e))?;

    let title = album.name.clone();
    let artists = album.artists.iter().map(|a| a.name.clone()).collect();

    let external_cover_url = album
        .cover
        .iter()
        .max_by_key(|thumb| thumb.width)
        .map(|thumb| thumb.url.clone());

    let release_year = album.year.map(|y| y as i32);

    let track_downloads: Vec<TrackDownload> = album
        .tracks
        .into_iter()
        .enumerate()
        .map(|(idx, track)| {
            let sanitized_name = sanitize(&track.name);

            TrackDownload {
                title: track.name.clone(),
                artists: track.artists.iter().map(|a| a.name.clone()).collect(),
                album: title.clone(),
                track_number: Some((idx + 1) as i32),
                genres: None,
                release_year,
                url: format!("https://music.youtube.com/watch?v={}", track.id),
                file_name: format!("{}.mp3", sanitized_name),
            }
        })
        .collect();

    if track_downloads.is_empty() {
        return Err("The album was fetched successfully, but contained 0 tracks.".to_string());
    }
    println!("track downloads {track_downloads:#?}");
    Ok(AlbumDownload {
        title,
        artists,
        tracks: track_downloads,
        genres: None,
        release_year,
        external_cover_url,
    })
}

fn extract_browse_id(browse_url: &str) -> Result<&str, String> {
    // Catch explicit playlist URLs early
    if browse_url.contains("playlist") || browse_url.contains("list=") {
        return Err("Unsupported URL type: Only YouTube Music browse/album URLs are supported, not playlists.".to_string());
    }

    if browse_url.contains("browse/") {
        browse_url
            .split("browse/")
            .last()
            .filter(|s| !s.is_empty()) // edge case: trailing slash with nothing after it
            .ok_or_else(|| "Invalid browse URL format".to_string())
    } else {
        Ok(browse_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_browse_id_pure_id() {
        let input = "MPREb_QGz4un0op6F";
        let result = extract_browse_id(input);
        assert_eq!(result, Ok("MPREb_QGz4un0op6F"));
    }

    #[test]
    fn test_extract_browse_id_full_url() {
        let input = "https://music.youtube.com/browse/MPREb_QGz4un0op6F";
        let result = extract_browse_id(input);
        assert_eq!(result, Ok("MPREb_QGz4un0op6F"));
    }

    #[test]
    fn test_extract_browse_id_malformed_url() {
        let input = "https://music.youtube.com/browse/";
        let result = extract_browse_id(input);
        assert!(
            result.is_err(),
            "Should return an error for trailing empty browse paths"
        );
    }

    #[test]
    fn test_extract_browse_id_rejects_playlists() {
        let input = "https://music.youtube.com/playlist?list=PLgeTf6bdBvoJJUr11oJVFnPd3XHNkt9tX";
        let result = extract_browse_id(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported URL type"));
    }
}

// pub async fn download_track_directly(
//     stream_url: &str,
//     output_dir: &str,
//     file_name: &str,
// ) -> Result<PathBuf, String> {
//     println!("[blacktape-debug] Starting direct track download...");
//     println!(
//         "[blacktape-debug] Target Dir: {}, File: {}",
//         output_dir, file_name
//     );
//
//     // 1. Create a client with a modern browser user-agent
//     let client = reqwest::Client::builder()
//         .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
//         .build()
//         .map_err(|e| e.to_string())?;
//
//     // 2. Add standard video-streaming fetch headers to look like a native media player asset request
//     let response = client
//         .get(stream_url)
//         .header("Accept", "*/*")
//         .header("Accept-Language", "en-US,en;q=0.9")
//         .header("Sec-Fetch-Mode", "cors")
//         .header("Sec-Fetch-Site", "cross-site")
//         // This is crucial: keeps the connection alive across chunk boundaries
//         .header("Connection", "keep-alive")
//         .send()
//         .await
//         .map_err(|e: reqwest::Error| format!("Network Request Failed: {}", e))?;
//
//     // 1. Debug HTTP Status Code
//     let status = response.status();
//     println!("[blacktape-debug] HTTP Response Status: {}", status);
//     if !status.is_success() {
//         eprintln!("[blacktape-debug] Google Video server rejected the stream URL request.");
//         return Err(format!("Server returned error status: {}", status));
//     }
//
//     let full_path = Path::new(output_dir).join(format!("{}.m4a", file_name));
//     println!("[blacktape-debug] Creating file at path: {:?}", full_path);
//
//     let mut file = File::create(&full_path)
//         .await
//         .map_err(|e: std::io::Error| {
//             let err_msg = format!("Failed to create local file: {}", e);
//             eprintln!("[blacktape-debug] {}", err_msg);
//             err_msg
//         })?;
//
//     let mut stream = response;
//     let mut total_bytes_written = 0;
//     let mut chunk_count = 0;
//
//     println!("[blacktape-debug] Beginning data stream loop...");
//
//     // 2. Track Chunk Arrival
//     while let Some(chunk) = stream.chunk().await.map_err(|e: reqwest::Error| {
//         let err_msg = format!("Error reading stream chunk: {}", e);
//         eprintln!("[blacktape-debug] {}", err_msg);
//         err_msg
//     })? {
//         chunk_count += 1;
//         let bytes: &[u8] = &chunk;
//         let chunk_size = bytes.len();
//
//         // 3. Track File Writes
//         file.write_all(bytes).await.map_err(|e: std::io::Error| {
//             let err_msg = format!("Error writing chunk to disk: {}", e);
//             eprintln!("[blacktape-debug] {}", err_msg);
//             err_msg
//         })?;
//
//         total_bytes_written += chunk_size;
//
//         // Print progress every 20 chunks so your terminal doesn't get utterly spammed
//         if chunk_count % 20 == 0 {
//             println!(
//                 "[blacktape-debug] Stream progress: Chunk #{}, Received {} KB (Total: {} MB)",
//                 chunk_count,
//                 chunk_size / 1024,
//                 total_bytes_written / (1024 * 1024)
//             );
//         }
//     }
//
//     // Flush remaining bytes to ensure everything is saved safely to disk
//     file.flush().await.map_err(|e| e.to_string())?;
//
//     println!(
//         "[blacktape-debug] Download complete! Saved {} total bytes to {:?}",
//         total_bytes_written, full_path
//     );
//
//     Ok(full_path)
// }
