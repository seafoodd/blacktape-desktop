use crate::download::{AlbumDownload, TrackDownload};
use crate::utils::sanitize_fs;
use regex::Regex;
use reqwest::Client;

pub async fn parse_album(album_url: &str) -> Result<AlbumDownload, String> {
    let base_url = extract_base_url(album_url)?;

    let client = Client::new();
    let html = client
        .get(album_url)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read page payload: {}", e))?;

    let mut tracks = parse_tracks_from_html(&html, base_url)?;
    let album_meta = parse_album_meta(&html);

    let album_artist_fallback = album_meta
        .artists
        .first()
        .cloned()
        .unwrap_or_else(|| "Unknown Artist".into());

    for (idx, track) in tracks.iter_mut().enumerate() {
        if let Some(clean_title) = track.file_name.splitn(2, ". ").nth(1) {
            track.title = clean_title.to_string();
        }
        track.album = album_meta.title.clone();
        track.artists = album_meta.artists.clone();
        track.album_artist = album_artist_fallback.clone();
        track.track_number = Some((idx + 1) as i32);
        track.genres = album_meta.genres.clone();
        track.release_year = album_meta.release_year;
    }

    Ok(AlbumDownload {
        title: album_meta.title,
        artists: album_meta.artists,
        album_artist: album_artist_fallback,
        tracks,
        genres: album_meta.genres,
        release_year: album_meta.release_year,
        external_cover_url: album_meta.external_cover_url,
    })
}

/// Parses a single Bandcamp track URL and returns a tuple containing
/// `(TrackDownload, Option<external_cover_url>)`.
pub async fn parse_track(track_url: &str) -> Result<(TrackDownload, Option<String>), String> {
    let client = Client::new();
    let html = client
        .get(track_url)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Failed to read page payload: {e}"))?;

    let meta = parse_track_meta(&html);
    let sanitized_name = sanitize_fs(&meta.title);

    let track = TrackDownload {
        title: meta.title,
        artists: meta.artists.clone(),
        album_artist: meta
            .artists
            .first()
            .cloned()
            .unwrap_or_else(|| "Unknown Artist".into()),
        album: meta.album,
        track_number: meta.track_number,
        genres: meta.genres,
        release_year: meta.release_year,
        url: track_url.to_string(),
        file_name: format!("{}.mp3", sanitized_name),
        source_item_id: None,
    };

    Ok((track, meta.external_cover_url))
}

struct TrackMeta {
    title: String,
    artists: Vec<String>,
    album: String,
    track_number: Option<i32>,
    genres: Option<Vec<String>>,
    release_year: Option<i32>,
    external_cover_url: Option<String>,
}

fn parse_track_meta(html: &str) -> TrackMeta {
    let mut title = None;
    let mut artists = Vec::new();
    let mut album = None;
    let mut track_number = None;
    let mut genres = None;
    let mut release_year = None;
    let mut external_cover_url = None;

    let json_ld = extract_json_ld_block(html).unwrap_or_default();

    // 1. EXTRACT TRACK TITLE & ARTIST NAME
    if let Some(caps) = Regex::new(r#"<meta[^>]*property="og:title"[^>]*content="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        let og_content = caps.get(1).unwrap().as_str();
        if let Some(idx) = og_content.rfind(", by ") {
            title = Some(og_content[..idx].to_string());
            artists.push(og_content[idx + 5..].to_string());
        } else {
            title = Some(og_content.to_string());
        }
    }

    if title.is_none() {
        if let Some(caps) = Regex::new(r#""name"\s*:\s*"([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(&json_ld))
        {
            title = Some(caps.get(1).unwrap().as_str().to_string());
        }
    }

    if artists.is_empty() {
        if let Some(caps) = Regex::new(r#""byArtist"\s*:\s*\{[^}]*"name"\s*:\s*"([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(&json_ld))
        {
            artists.push(caps.get(1).unwrap().as_str().to_string());
        }
    }

    // 2. EXTRACT ALBUM NAME & TRACK NUMBER
    if let Some(caps) = Regex::new(r#""inAlbum"\s*:\s*\{[^}]*"name"\s*:\s*"([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        album = Some(caps.get(1).unwrap().as_str().to_string());
    }

    if let Some(caps) = Regex::new(r#""trackNum(?:ber)?"\s*:\s*(\d+)"#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        track_number = caps.get(1).unwrap().as_str().parse::<i32>().ok();
    }

    // 3. EXTRACT COVER ART
    if let Some(caps) = Regex::new(r#"<meta[^>]*property="og:image"[^>]*content="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        external_cover_url = Some(maximize_cover_url(caps.get(1).unwrap().as_str()));
    } else if let Some(caps) = Regex::new(r#"<link[^>]*rel="image_src"[^>]*href="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        external_cover_url = Some(maximize_cover_url(caps.get(1).unwrap().as_str()));
    }

    // 4. EXTRACT RELEASE YEAR
    if let Some(caps) = Regex::new(r#""datePublished"\s*:\s*"[^"]*?(\d{4})""#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        release_year = caps.get(1).unwrap().as_str().parse::<i32>().ok();
    } else if let Some(caps) = Regex::new(r#"(?i)released\s+(?:\d{1,2}\s+[a-z]+\s+)?\b(\d{4})\b"#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        release_year = caps.get(1).unwrap().as_str().parse::<i32>().ok();
    }

    // 5. EXTRACT GENRES
    if let Some(caps) = Regex::new(r#""keywords"\s*:\s*\[([^\]]+)\]"#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        let keywords_block = caps.get(1).unwrap().as_str();
        let mut tags = Vec::new();
        if let Some(kw_re) = Regex::new(r#""([^"]+)""#).ok() {
            for kw_cap in kw_re.captures_iter(keywords_block) {
                tags.push(kw_cap.get(1).unwrap().as_str().to_string());
            }
        }
        if !tags.is_empty() {
            genres = Some(tags);
        }
    }

    TrackMeta {
        title: title.unwrap_or_else(|| "Unknown Track".to_string()),
        artists: if artists.is_empty() {
            vec!["Unknown Artist".to_string()]
        } else {
            artists
        },
        album: album.unwrap_or_else(|| "Singles".to_string()),
        track_number,
        genres,
        release_year,
        external_cover_url,
    }
}

fn extract_base_url(album_url: &str) -> Result<&str, String> {
    if let Some(end_idx) = album_url.find(".com") {
        Ok(&album_url[..end_idx + 4])
    } else {
        Err("Invalid domain structure for Bandcamp parsing".to_string())
    }
}

fn parse_tracks_from_html(html: &str, base_url: &str) -> Result<Vec<TrackDownload>, String> {
    let track_regex = Regex::new(
        r#"(?s)href="(/track/[^"\s]+)"[^>]*>\s*<span class="track-title">([^<]+)</span>"#,
    )
    .map_err(|e| e.to_string())?;

    let mut tracks = Vec::new();
    let mut track_count = 1;

    for caps in track_regex.captures_iter(html) {
        let relative_path = caps.get(1).map_or("", |m| m.as_str());
        let track_name = caps.get(2).map_or("Unknown Track", |m| m.as_str()).trim();

        tracks.push(TrackDownload {
            url: format!("{}{}", base_url, relative_path),
            file_name: format!("{:02}. {}", track_count, track_name),
            title: "".to_string(),
            artists: vec![],
            album_artist: "".to_string(),
            album: "".to_string(),
            track_number: None,
            genres: None,
            release_year: None,
            source_item_id: None,
        });

        track_count += 1;
    }

    if tracks.is_empty() {
        return Err("Could not find any tracks inside the target HTML layout".to_string());
    }

    Ok(tracks)
}

struct AlbumMeta {
    title: String,
    artists: Vec<String>,
    genres: Option<Vec<String>>,
    release_year: Option<i32>,
    external_cover_url: Option<String>,
}

fn parse_album_meta(html: &str) -> AlbumMeta {
    let mut title = None;
    let mut artists = Vec::new();
    let mut genres = None;
    let mut release_year = None;
    let mut external_cover_url = None;

    // Isolate the structured script block safely if present
    let json_ld = extract_json_ld_block(html).unwrap_or_default();

    // 1. EXTRACT ALBUM TITLE & ARTIST NAME
    // Layer A: Extract from universal Open Graph metadata (highly uniform format: "Title, by Artist")
    if let Some(caps) = Regex::new(r#"<meta[^>]*property="og:title"[^>]*content="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        let og_content = caps.get(1).unwrap().as_str();
        if let Some(idx) = og_content.rfind(", by ") {
            title = Some(og_content[..idx].to_string());
            artists.push(og_content[idx + 5..].to_string());
        } else {
            title = Some(og_content.to_string());
        }
    }

    // Layer B Fallback: Extract from structural Schema JSON-LD metadata
    if title.is_none() {
        if let Some(caps) = Regex::new(r#""name"\s*:\s*"([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(&json_ld))
        {
            title = Some(caps.get(1).unwrap().as_str().to_string());
        }
    }
    if artists.is_empty() {
        if let Some(caps) = Regex::new(r#""byArtist"\s*:\s*\{[^}]*"name"\s*:\s*"([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(&json_ld))
        {
            artists.push(caps.get(1).unwrap().as_str().to_string());
        }
    }

    // Layer C Fallback: Document `<title>` parsing
    if title.is_none() || artists.is_empty() {
        if let Some(caps) = Regex::new(r#"<title>\s*([^|]+?)\s*\|\s*([^<]+?)\s*</title>"#)
            .ok()
            .and_then(|r| r.captures(html))
        {
            if title.is_none() {
                title = Some(caps.get(1).unwrap().as_str().trim().to_string());
            }
            if artists.is_empty() {
                artists.push(caps.get(2).unwrap().as_str().trim().to_string());
            }
        }
    }

    // 2. EXTRACT EXTERNAL COVER URL
    if let Some(caps) = Regex::new(r#"<meta[^>]*property="og:image"[^>]*content="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        external_cover_url = Some(maximize_cover_url(caps.get(1).unwrap().as_str()));
    } else if let Some(caps) = Regex::new(r#"<link[^>]*rel="image_src"[^>]*href="([^"]+)""#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        external_cover_url = Some(maximize_cover_url(caps.get(1).unwrap().as_str()));
    }

    // 3. EXTRACT RELEASE YEAR
    if let Some(caps) = Regex::new(r#""datePublished"\s*:\s*"[^"]*?(\d{4})""#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        release_year = caps.get(1).unwrap().as_str().parse::<u32>().ok();
    } else if let Some(caps) = Regex::new(r#"(?i)released\s+(?:\d{1,2}\s+[a-z]+\s+)?\b(\d{4})\b"#)
        .ok()
        .and_then(|r| r.captures(html))
    {
        release_year = caps.get(1).unwrap().as_str().parse::<u32>().ok();
    }

    // 4. EXTRACT GENRES (KEYWORDS)
    if let Some(caps) = Regex::new(r#""keywords"\s*:\s*\[([^\]]+)\]"#)
        .ok()
        .and_then(|r| r.captures(&json_ld))
    {
        let keywords_block = caps.get(1).unwrap().as_str();
        let mut tags = Vec::new();
        if let Some(kw_re) = Regex::new(r#""([^"]+)""#).ok() {
            for kw_cap in kw_re.captures_iter(keywords_block) {
                tags.push(kw_cap.get(1).unwrap().as_str().to_string());
            }
        }
        if !tags.is_empty() {
            genres = Some(tags);
        }
    }

    AlbumMeta {
        title: title.unwrap_or_else(|| "Unknown Album".to_string()),
        artists: if artists.is_empty() {
            vec!["Unknown Artist".to_string()]
        } else {
            artists
        },
        genres,
        release_year: release_year.map(|y| y as i32),
        external_cover_url,
    }
}

fn extract_json_ld_block(html: &str) -> Option<String> {
    let re = Regex::new(r#"(?s)<script\s+type="application/ld\+json">([\s\S]*?)</script>"#).ok()?;
    re.captures(html)
        .map(|caps| caps.get(1).unwrap().as_str().to_string())
}

fn maximize_cover_url(url: &str) -> String {
    let re = Regex::new(r"_\d+\.(jpg|jpeg|png)").unwrap();
    re.replace(url, "_0.jpg").to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_base_url_success() {
        let url = "https://artistname.bandcamp.com/album/some-album";
        assert_eq!(extract_base_url(url), Ok("https://artistname.bandcamp.com"));
    }

    #[test]
    fn test_extract_base_url_malformed() {
        let url = "https://invalid-url-structure";
        assert!(extract_base_url(url).is_err());
    }

    #[test]
    fn test_parse_tracks_from_html_no_matches() {
        let empty_html = "<html><body><h1>No tracks here!</h1></body></html>";
        let result = parse_tracks_from_html(empty_html, "https://test.bandcamp.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_tracks_from_html_success() {
        // Using real raw HTML pulled directly from Bandcamp
        let mock_html = r#"
            <tbody>
                <tr class="track_row_view linked" rel="tracknum=1">
                    <td class="play-col"><a role="button" aria-label="Play In Absentia ΛΟΓΟΣ"><div class="play_status"></div></a></td>
                    <td class="track-number-col"><div class="track_number secondaryText">1.</div></td>
                    <td class="title-col">
                        <div class="title">
                            <a href="/track/in-absentia-2"><span class="track-title">In Absentia ΛΟΓΟΣ</span></a>
                            <span class="time secondaryText">04:33</span>
                        </div>
                    </td>
                </tr>
                <tr class="track_row_view linked" rel="tracknum=2">
                    <td class="play-col"><a role="button" aria-label="Play Spiral Out (Keep Going)"><div class="play_status"></div></a></td>
                    <td class="track-number-col"><div class="track_number secondaryText">2.</div></td>
                    <td class="title-col">
                        <div class="title">
                            <a href="/track/spiral-out-keep-going-2"><span class="track-title">Spiral Out (Keep Going)</span></a>
                            <span class="time secondaryText">04:38</span>
                        </div>
                    </td>
                </tr>
            </tbody>
        "#;

        let base_url = "https://test.bandcamp.com";
        let result = parse_tracks_from_html(mock_html, base_url);

        assert!(result.is_ok(), "Failed to parse tracks: {:?}", result.err());
        let tracks = result.unwrap();

        assert_eq!(tracks.len(), 2);

        assert_eq!(
            tracks[0].url,
            "https://test.bandcamp.com/track/in-absentia-2"
        );
        assert_eq!(tracks[0].file_name, "01. In Absentia ΛΟΓΟΣ");

        assert_eq!(
            tracks[1].url,
            "https://test.bandcamp.com/track/spiral-out-keep-going-2"
        );
        assert_eq!(tracks[1].file_name, "02. Spiral Out (Keep Going)");
    }
}
