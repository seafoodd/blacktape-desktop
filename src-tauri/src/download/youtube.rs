use crate::download::{ytdlp, AlbumDownload, TrackDownload};
use crate::utils::sanitize_fs;
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
    let artists: Vec<String> = album.artists.iter().map(|a| a.name.clone()).collect();
    let album_artist = artists
        .first()
        .cloned()
        .unwrap_or_else(|| "Unknown Artist".into());

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
            let sanitized_name = sanitize_fs(&track.name);

            TrackDownload {
                title: track.name.clone(),
                artists: track.artists.iter().map(|a| a.name.clone()).collect(),
                album_artist: album_artist.clone(),
                album: title.clone(),
                track_number: Some((idx + 1) as i32),
                genres: None,
                release_year,
                url: format!("https://music.youtube.com/watch?v={}", track.id),
                file_name: format!("{}.mp3", sanitized_name),
                source_item_id: Some(track.id.clone()),
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
        album_artist,
        tracks: track_downloads,
        genres: None,
        release_year,
        external_cover_url,
    })
}

fn extract_browse_id(browse_url: &str) -> Result<&str, String> {
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
