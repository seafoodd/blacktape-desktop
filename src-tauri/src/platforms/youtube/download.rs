use crate::download::{AlbumDownload, TrackDownload};
use crate::utils::sanitize_fs;
use rustypipe::client::RustyPipe;

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

/// YouTube Music reports track lengths in whole seconds; 0 or missing means unknown.
fn secs_to_ms(secs: Option<u32>) -> Option<u64> {
    secs.filter(|s| *s > 0).map(|s| u64::from(s) * 1000)
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
    match rp.query().music_album(browse_id).await {
        Ok(album) => {
            let title = album.name.clone();
            let artists: Vec<String> = album.artists.iter().map(|a| a.name.clone()).collect();
            let album_artist = artists
                .first()
                .cloned()
                .unwrap_or_else(|| "Various Artists".into());

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
                        disc_number: None,
                        genres: None,
                        release_year,
                        url: format!("https://music.youtube.com/watch?v={}", track.id),
                        file_name: format!("{}.mp3", sanitized_name),
                        source_item_id: Some(track.id.clone()),
                        duration_ms: secs_to_ms(track.duration),
                        fallback_urls: Vec::new(),
                        ..Default::default()
                    }
                })
                .collect();

            if track_downloads.is_empty() {
                return Err("The album was fetched successfully, but contained 0 tracks.".to_string());
            }
            tracing::debug!(count = track_downloads.len(), "Extracted track downloads for YouTube album");
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
        Err(rp_err) => {
            tracing::warn!(browse_id = %browse_id, error = %rp_err, "RustyPipe music_album failed, trying InnerTube browse fallback");
            parse_album_innertube(browse_id)
                .await
                .map_err(|_| format!("RustyPipe failed to fetch album: {}", rp_err))
        }
    }
}

async fn parse_album_innertube(browse_id: &str) -> Result<AlbumDownload, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(4))
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": super::search_fast::CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "browseId": browse_id
    });
    let resp = client
        .post("https://music.youtube.com/youtubei/v1/browse?prettyPrint=false")
        .header("Origin", "https://music.youtube.com")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0",
        )
        .header("Cookie", "SOCS=CAI")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    parse_innertube_album_json(&json)
}

fn runs_text(v: &serde_json::Value) -> String {
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect::<String>()
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn parse_duration_ms_str(s: &str) -> Option<u64> {
    let mut parts = s.trim().split(':').rev();
    let secs = parts.next()?.parse::<u64>().ok()?;
    let mins = parts.next()?.parse::<u64>().ok()?;
    let hours = parts.next().and_then(|h| h.parse::<u64>().ok()).unwrap_or(0);
    let total = hours * 3600 + mins * 60 + secs;
    (total > 0).then_some(total * 1000)
}

fn parse_innertube_album_json(json: &serde_json::Value) -> Result<AlbumDownload, String> {
    let two_col = json.pointer("/contents/twoColumnBrowseResultsRenderer");
    let header = two_col
        .and_then(|tc| tc.pointer("/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer"))
        .or_else(|| json.pointer("/header/musicDetailHeaderRenderer"));

    let title = header
        .and_then(|h| h.get("title"))
        .map(runs_text)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing album title in InnerTube browse response".to_string())?;

    let mut artists: Vec<String> = header
        .and_then(|h| h.pointer("/straplineTextOne/runs"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                .map(str::trim)
                .filter(|t| !t.is_empty() && *t != "•" && *t != "," && *t != "&")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let subtitle = header
        .and_then(|h| h.get("subtitle"))
        .map(runs_text)
        .unwrap_or_default();
    let release_year = subtitle
        .split('•')
        .map(str::trim)
        .find_map(|part| (part.len() == 4).then(|| part.parse::<i32>().ok()).flatten());
    if artists.is_empty() {
        for part in subtitle.split('•').map(str::trim) {
            if !part.is_empty()
                && !matches!(part, "Album" | "EP" | "Single")
                && part.parse::<i32>().is_err()
            {
                artists.push(part.to_string());
                break;
            }
        }
    }

    let album_artist = artists
        .first()
        .cloned()
        .unwrap_or_else(|| "Various Artists".into());

    let external_cover_url = header
        .and_then(|h| {
            h.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                .or_else(|| h.pointer("/thumbnail/croppedSquareThumbnailRenderer/thumbnail/thumbnails"))
        })
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url").and_then(|u| u.as_str()))
        .map(maximize_cover_url);

    let shelf_items = two_col
        .and_then(|tc| tc.pointer("/secondaryContents/sectionListRenderer/contents/0/musicShelfRenderer/contents"))
        .or_else(|| json.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicShelfRenderer/contents"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| "Missing tracklist in InnerTube browse response".to_string())?;

    let mut tracks = Vec::new();
    for (idx, item) in shelf_items.iter().enumerate() {
        let Some(row) = item.get("musicResponsiveListItemRenderer") else {
            continue;
        };
        let video_id = row
            .pointer("/playlistItemData/videoId")
            .or_else(|| row.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId"))
            .or_else(|| row.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let Some(video_id) = video_id else {
            continue;
        };
        let track_title = row
            .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text")
            .map(runs_text)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Track {}", idx + 1));
        let duration_ms = row
            .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
            .map(runs_text)
            .and_then(|s| parse_duration_ms_str(&s));
        let sanitized_name = sanitize_fs(&track_title);

        tracks.push(TrackDownload {
            title: track_title,
            artists: artists.clone(),
            album_artist: album_artist.clone(),
            album: title.clone(),
            track_number: Some((tracks.len() + 1) as i32),
            disc_number: None,
            genres: None,
            release_year,
            url: format!("https://music.youtube.com/watch?v={video_id}"),
            file_name: format!("{sanitized_name}.mp3"),
            source_item_id: Some(video_id.to_string()),
            duration_ms,
            fallback_urls: Vec::new(),
            ..Default::default()
        });
    }

    if tracks.is_empty() {
        return Err("InnerTube browse response contained 0 tracks.".to_string());
    }
    Ok(AlbumDownload {
        title,
        artists,
        album_artist,
        tracks,
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
        let duration_ms = secs_to_ms(track_item.duration);
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
            disc_number: None,
            genres: None,
            release_year,
            url: format!("https://music.youtube.com/watch?v={}", video_id),
            file_name: format!("{}.mp3", sanitized_name),
            source_item_id: Some(video_id),
            duration_ms,
            fallback_urls: Vec::new(),
            ..Default::default()
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
        disc_number: None,
        genres: None,
        release_year,
        url: format!("https://youtube.com/watch?v={}", video_id),
        file_name: format!("{}.mp3", sanitized_name),
        source_item_id: Some(video_id),
        duration_ms: None,
        fallback_urls: Vec::new(),
        ..Default::default()
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
    fn secs_to_ms_converts_seconds_and_treats_zero_as_unknown() {
        assert_eq!(secs_to_ms(Some(185)), Some(185_000));
        assert_eq!(secs_to_ms(Some(1)), Some(1_000));
        assert_eq!(secs_to_ms(Some(u32::MAX)), Some(u64::from(u32::MAX) * 1000));
        assert_eq!(secs_to_ms(Some(0)), None);
        assert_eq!(secs_to_ms(None), None);
    }

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

    #[test]
    fn parse_innertube_album_json_extracts_mixed_album_tracks() {
        let json = serde_json::json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicResponsiveHeaderRenderer": {
                                            "title": { "runs": [{ "text": "Lethal Injection" }] },
                                            "straplineTextOne": { "runs": [{ "text": "Ice Cube" }] },
                                            "subtitle": { "runs": [{ "text": "Album" }, { "text": " • " }, { "text": "1993" }] },
                                            "thumbnail": {
                                                "musicThumbnailRenderer": {
                                                    "thumbnail": { "thumbnails": [{ "url": "https://lh3.googleusercontent.com/img=w544-h544" }] }
                                                }
                                            }
                                        }
                                    }]
                                }
                            }
                        }
                    }],
                    "secondaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "musicShelfRenderer": {
                                    "contents": [{
                                        "musicResponsiveListItemRenderer": {
                                            "playlistItemData": { "videoId": "abc12345678" },
                                            "flexColumns": [{
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "You Know How We Do It" }] }
                                                }
                                            }],
                                            "fixedColumns": [{
                                                "musicResponsiveListItemFixedColumnRenderer": {
                                                    "text": { "runs": [{ "text": "3:52" }] }
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        });

        let album = parse_innertube_album_json(&json).expect("parsed album");
        assert_eq!(album.title, "Lethal Injection");
        assert_eq!(album.album_artist, "Ice Cube");
        assert_eq!(album.release_year, Some(1993));
        assert_eq!(album.tracks.len(), 1);
        assert_eq!(album.tracks[0].title, "You Know How We Do It");
        assert_eq!(album.tracks[0].duration_ms, Some(232_000));
        assert_eq!(album.tracks[0].url, "https://music.youtube.com/watch?v=abc12345678");
    }
}
