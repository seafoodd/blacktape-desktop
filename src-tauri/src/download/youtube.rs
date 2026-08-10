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

/// Extracts the YouTube 11-character video ID from various URL formats or raw ID strings.
pub fn extract_video_id(url: &str) -> Result<String, String> {
    if url.contains("v=") {
        let id_part = url.split("v=").nth(1).unwrap_or("");
        let id = id_part.split('&').next().unwrap_or(id_part);
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }

    if url.contains("youtu.be/") {
        let id_part = url.split("youtu.be/").last().unwrap_or("");
        let id = id_part.split('?').next().unwrap_or(id_part);
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }

    // Direct video ID string
    if !url.contains('/') && !url.contains('?') && !url.is_empty() {
        return Ok(url.to_string());
    }

    Err("Invalid YouTube track URL or video ID format.".to_string())
}

fn maximize_cover_url(url: &str) -> String {
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        // Strip existing size/quality modifiers after '=' and request 1600x1600 JPEG at quality 90
        let base_url = url.split('=').next().unwrap_or(url);
        format!("{base_url}=w1600-h1600-l90-rj")
    } else if url.contains("i.ytimg.com") {
        url.replace("hqdefault.jpg", "maxresdefault.jpg")
            .replace("sddefault.jpg", "maxresdefault.jpg")
            .replace("mqdefault.jpg", "maxresdefault.jpg")
            .replace("hq720.jpg", "maxresdefault.jpg")
    } else {
        url.to_string()
    }
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
        .map(|thumb| maximize_cover_url(&thumb.url));

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

/// Parses a single YouTube track or video URL and returns a tuple containing
/// `(TrackDownload, Option<external_cover_url>)`.
pub async fn parse_track(url: &str) -> Result<(TrackDownload, Option<String>), String> {
    let video_id = extract_video_id(url)?;
    let rp = RustyPipe::new();

    // 1. Attempt YouTube Music details fetch (richest metadata: album, artist list, release year)
    if let Ok(details) = rp.query().music_details(&video_id).await {
        let track_item = details.track;
        let title = track_item.name.clone();
        let artists: Vec<String> = track_item.artists.iter().map(|a| a.name.clone()).collect();
        let album_artist = artists
            .first()
            .cloned()
            .unwrap_or_else(|| "Unknown Artist".into());

        let mut release_year = None;
        if let Some(ref album_id) = track_item.album {
            if let Ok(album_data) = rp.query().music_album(&album_id.id).await {
                release_year = album_data.year.map(|y| y as i32);
            }
        }

        let album_name = track_item
            .album
            .map(|a| a.name)
            .unwrap_or_else(|| "Singles".into());

        let external_cover_url = track_item
            .cover
            .iter()
            .max_by_key(|t| t.width)
            .map(|t| maximize_cover_url(&t.url));

        let sanitized_name = sanitize_fs(&title);

        let track = TrackDownload {
            title,
            artists,
            album_artist,
            album: album_name,
            track_number: track_item.track_nr.map(|n| n as i32),
            genres: None,
            release_year,
            url: format!("https://music.youtube.com/watch?v={}", video_id),
            file_name: format!("{}.mp3", sanitized_name),
            source_item_id: Some(video_id),
        };

        return Ok((track, external_cover_url));
    }

    // 2. Fallback to standard YouTube video details if YouTube Music fails
    let video = rp
        .query()
        .video_details(&video_id)
        .await
        .map_err(|e| format!("Failed to fetch YouTube video details: {e}"))?;

    let title = video.name;
    let artist = video.channel.name;
    let release_year = video.publish_date.map(|d| d.year());
    let external_cover_url = Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id));

    let sanitized_name = sanitize_fs(&title);

    let track = TrackDownload {
        title,
        artists: vec![artist.clone()],
        album_artist: artist,
        album: "Singles".into(),
        track_number: None,
        genres: None,
        release_year,
        url: format!("https://youtube.com/watch?v={}", video_id),
        file_name: format!("{}.mp3", sanitized_name),
        source_item_id: Some(video_id),
    };

    Ok((track, external_cover_url))
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
