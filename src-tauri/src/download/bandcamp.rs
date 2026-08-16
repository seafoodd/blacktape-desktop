use crate::download::{AlbumDownload, TrackDownload};
use crate::utils::sanitize_fs;
use regex::Regex;
use reqwest::Client;
use std::sync::OnceLock;

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

struct AlbumMeta {
    title: String,
    artists: Vec<String>,
    genres: Option<Vec<String>>,
    release_year: Option<i32>,
    external_cover_url: Option<String>,
}

fn parse_track_meta(html: &str) -> TrackMeta {
    let json_ld = extract_json_ld_block(html).unwrap_or_default();
    let (og_title, mut artists) = extract_og_title_and_artist(html);

    let title = og_title
        .or_else(|| extract_json_ld_name(&json_ld))
        .unwrap_or_else(|| "Unknown Track".to_string());

    if artists.is_empty() {
        if let Some(artist) = extract_json_ld_artist(&json_ld) {
            artists.push(artist);
        } else {
            artists.push("Unknown Artist".to_string());
        }
    }

    TrackMeta {
        title,
        artists,
        album: extract_json_ld_in_album(&json_ld).unwrap_or_else(|| "Singles".to_string()),
        track_number: extract_track_number(&json_ld),
        genres: extract_genres(&json_ld),
        release_year: extract_release_year(&json_ld, html),
        external_cover_url: extract_cover_url(html),
    }
}

fn parse_album_meta(html: &str) -> AlbumMeta {
    let json_ld = extract_json_ld_block(html).unwrap_or_default();
    let (og_title, mut artists) = extract_og_title_and_artist(html);

    let mut title = og_title.or_else(|| extract_json_ld_name(&json_ld));

    if artists.is_empty() {
        if let Some(artist) = extract_json_ld_artist(&json_ld) {
            artists.push(artist);
        }
    }

    if title.is_none() || artists.is_empty() {
        if let Some((doc_title, doc_artist)) = extract_html_title_tag(html) {
            if title.is_none() {
                title = Some(doc_title);
            }
            if artists.is_empty() {
                artists.push(doc_artist);
            }
        }
    }

    AlbumMeta {
        title: title.unwrap_or_else(|| "Unknown Album".to_string()),
        artists: if artists.is_empty() {
            vec!["Unknown Artist".to_string()]
        } else {
            artists
        },
        genres: extract_genres(&json_ld),
        release_year: extract_release_year(&json_ld, html),
        external_cover_url: extract_cover_url(html),
    }
}

fn extract_og_title_and_artist(html: &str) -> (Option<String>, Vec<String>) {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"<meta[^>]*property="og:title"[^>]*content="([^"]+)""#).unwrap()
    });

    if let Some(caps) = re.captures(html) {
        let og_content = caps.get(1).unwrap().as_str();
        if let Some(idx) = og_content.rfind(", by ") {
            return (
                Some(og_content[..idx].to_string()),
                vec![og_content[idx + 5..].to_string()],
            );
        }
        return (Some(og_content.to_string()), Vec::new());
    }
    (None, Vec::new())
}

fn extract_json_ld_name(json_ld: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""name"\s*:\s*"([^"]+)""#).unwrap());
    re.captures(json_ld)
        .map(|caps| caps.get(1).unwrap().as_str().to_string())
}

fn extract_json_ld_artist(json_ld: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r#""byArtist"\s*:\s*\{[^}]*"name"\s*:\s*"([^"]+)""#).unwrap());
    re.captures(json_ld)
        .map(|caps| caps.get(1).unwrap().as_str().to_string())
}

fn extract_json_ld_in_album(json_ld: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r#""inAlbum"\s*:\s*\{[^}]*"name"\s*:\s*"([^"]+)""#).unwrap());
    re.captures(json_ld)
        .map(|caps| caps.get(1).unwrap().as_str().to_string())
}

fn extract_track_number(json_ld: &str) -> Option<i32> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""trackNum(?:ber)?"\s*:\s*(\d+)"#).unwrap());
    re.captures(json_ld)
        .and_then(|caps| caps.get(1).unwrap().as_str().parse::<i32>().ok())
}

fn extract_html_title_tag(html: &str) -> Option<(String, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r#"<title>\s*([^|]+?)\s*\|\s*([^<]+?)\s*</title>"#).unwrap());
    re.captures(html).map(|caps| {
        (
            caps.get(1).unwrap().as_str().trim().to_string(),
            caps.get(2).unwrap().as_str().trim().to_string(),
        )
    })
}

fn extract_cover_url(html: &str) -> Option<String> {
    static OG_RE: OnceLock<Regex> = OnceLock::new();
    static LINK_RE: OnceLock<Regex> = OnceLock::new();

    let og_re = OG_RE.get_or_init(|| {
        Regex::new(r#"<meta[^>]*property="og:image"[^>]*content="([^"]+)""#).unwrap()
    });
    let link_re = LINK_RE
        .get_or_init(|| Regex::new(r#"<link[^>]*rel="image_src"[^>]*href="([^"]+)""#).unwrap());

    if let Some(caps) = og_re.captures(html) {
        Some(maximize_cover_url(caps.get(1).unwrap().as_str()))
    } else if let Some(caps) = link_re.captures(html) {
        Some(maximize_cover_url(caps.get(1).unwrap().as_str()))
    } else {
        None
    }
}

fn extract_release_year(json_ld: &str, html: &str) -> Option<i32> {
    static JSON_RE: OnceLock<Regex> = OnceLock::new();
    static HTML_RE: OnceLock<Regex> = OnceLock::new();

    let json_re =
        JSON_RE.get_or_init(|| Regex::new(r#""datePublished"\s*:\s*"[^"]*?(\d{4})""#).unwrap());
    let html_re = HTML_RE.get_or_init(|| {
        Regex::new(r#"(?i)released\s+(?:\d{1,2}\s+[a-z]+\s+)?\b(\d{4})\b"#).unwrap()
    });

    if let Some(caps) = json_re.captures(json_ld) {
        caps.get(1).unwrap().as_str().parse::<i32>().ok()
    } else if let Some(caps) = html_re.captures(html) {
        caps.get(1).unwrap().as_str().parse::<i32>().ok()
    } else {
        None
    }
}

fn extract_genres(json_ld: &str) -> Option<Vec<String>> {
    static BLOCK_RE: OnceLock<Regex> = OnceLock::new();
    static ITEM_RE: OnceLock<Regex> = OnceLock::new();

    let block_re = BLOCK_RE.get_or_init(|| Regex::new(r#""keywords"\s*:\s*\[([^]]+)\]"#).unwrap());
    let item_re = ITEM_RE.get_or_init(|| Regex::new(r#""([^"]+)""#).unwrap());

    let caps = block_re.captures(json_ld)?;
    let keywords_block = caps.get(1)?.as_str();

    let tags: Vec<String> = item_re
        .captures_iter(keywords_block)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();

    if tags.is_empty() {
        None
    } else {
        Some(tags)
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
    static TRACK_RE: OnceLock<Regex> = OnceLock::new();
    let track_regex = TRACK_RE.get_or_init(|| {
        Regex::new(
            r#"(?s)href="(/track/[^"\s]+)"[^>]*>\s*<span class="track-title">([^<]+)</span>"#,
        )
        .unwrap()
    });

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

fn extract_json_ld_block(html: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?s)<script\s+type="application/ld\+json">([\s\S]*?)</script>"#).unwrap()
    });
    re.captures(html)
        .map(|caps| caps.get(1).unwrap().as_str().to_string())
}

fn maximize_cover_url(url: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"_\d+\.(jpg|jpeg|png)").unwrap());
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
