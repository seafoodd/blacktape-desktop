use crate::download::{AlbumDownload, TrackDownload};
use crate::utils::sanitize_fs;
use regex::Regex;
use reqwest::Client;
use std::collections::HashMap;
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

/// Page url of the album a track page belongs to (json-ld `inAlbum` id/url, else the first `/album/` link of the
/// page), or None for a single. Relative links are resolved against the track page.
pub fn extract_album_url(html: &str, track_url: &str) -> Option<String> {
    static IN_ALBUM: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let in_album = IN_ALBUM.get_or_init(|| {
        Regex::new(r#""inAlbum"\s*:\s*\{[^}]*"(?:@id|url)"\s*:\s*"([^"]+/album/[^"]+)""#).unwrap()
    });
    let link = LINK.get_or_init(|| Regex::new(r#"href="((?:https?://[^"/]+)?/album/[^"?#]+)""#).unwrap());
    let found = in_album
        .captures(html)
        .or_else(|| link.captures(html))
        .map(|c| c[1].to_string())?;
    if found.starts_with('/') {
        Some(format!("{}{}", extract_base_url(track_url).ok()?, found))
    } else {
        Some(found)
    }
}

/// Fetches the track page and returns its album url (see `extract_album_url`).
pub async fn find_album_url(track_url: &str) -> Option<String> {
    let html = Client::new().get(track_url).send().await.ok()?.text().await.ok()?;
    extract_album_url(&html, track_url)
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
    let source_item_id = extract_track_id_from_html(&html, Some(track_url));
    let duration_ms = extract_single_track_duration_ms(&html, track_url);

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
        source_item_id,
        duration_ms,
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

    let block_re = BLOCK_RE.get_or_init(|| Regex::new(r#""keywords"\s*:\s*\[([^]]+)]"#).unwrap());
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

    let track_id_map = extract_album_track_ids(html);
    let duration_map = extract_album_track_durations_ms(html);

    let mut tracks = Vec::new();
    let mut track_count = 1;

    for caps in track_regex.captures_iter(html) {
        let relative_path = caps.get(1).map_or("", |m| m.as_str());
        let track_name = caps.get(2).map_or("Unknown Track", |m| m.as_str()).trim();

        let source_item_id = track_id_map.get(relative_path).cloned().or_else(|| {
            let slug = relative_path
                .trim_start_matches("/track/")
                .trim_matches('/');
            if !slug.is_empty() {
                Some(slug.to_string())
            } else {
                None
            }
        });

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
            source_item_id,
            duration_ms: duration_map.get(relative_path).copied(),
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

fn extract_track_id_from_html(html: &str, track_url: Option<&str>) -> Option<String> {
    static COMMENT_RE: OnceLock<Regex> = OnceLock::new();
    let comment_re =
        COMMENT_RE.get_or_init(|| Regex::new(r#"(?i)<!--\s*track id\s+(\d+)\s*-->"#).unwrap());
    if let Some(caps) = comment_re.captures(html) {
        return Some(caps.get(1).unwrap().as_str().to_string());
    }

    static ITEM_ID_RE: OnceLock<Regex> = OnceLock::new();
    let item_id_re =
        ITEM_ID_RE.get_or_init(|| Regex::new(r#"(?i)data-item-id="track-(\d+)""#).unwrap());
    if let Some(caps) = item_id_re.captures(html) {
        return Some(caps.get(1).unwrap().as_str().to_string());
    }

    static JSON_ID_RE: OnceLock<Regex> = OnceLock::new();
    let json_id_re = JSON_ID_RE
        .get_or_init(|| Regex::new(r#"(?i)["'](?:track_id|item_id)["']\s*:\s*(\d+)"#).unwrap());
    if let Some(caps) = json_id_re.captures(html) {
        return Some(caps.get(1).unwrap().as_str().to_string());
    }

    static EMBED_RE: OnceLock<Regex> = OnceLock::new();
    let embed_re =
        EMBED_RE.get_or_init(|| Regex::new(r#"(?i)(?:/track=|\btrack_id=)(\d+)"#).unwrap());
    if let Some(caps) = embed_re.captures(html) {
        return Some(caps.get(1).unwrap().as_str().to_string());
    }

    if let Some(url) = track_url {
        if let Some(idx) = url.find("/track/") {
            let slug = &url[idx + 7..];
            let clean_slug = slug
                .split(['?', '#'])
                .next()
                .unwrap_or(slug)
                .trim_matches('/');
            if !clean_slug.is_empty() {
                return Some(clean_slug.to_string());
            }
        }
    }

    None
}

fn extract_album_track_ids(html: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    static TRACKINFO_PAIR_RE1: OnceLock<Regex> = OnceLock::new();
    let re1 = TRACKINFO_PAIR_RE1.get_or_init(|| {
        Regex::new(r#"(?s)"id"\s*:\s*(\d+)[^}]*?"title_link"\s*:\s*"(/track/[^"\s]+)""#).unwrap()
    });
    for caps in re1.captures_iter(html) {
        let id = caps.get(1).unwrap().as_str().to_string();
        let link = caps.get(2).unwrap().as_str().to_string();
        map.insert(link, id);
    }

    static TRACKINFO_PAIR_RE2: OnceLock<Regex> = OnceLock::new();
    let re2 = TRACKINFO_PAIR_RE2.get_or_init(|| {
        Regex::new(r#"(?s)"title_link"\s*:\s*"(/track/[^"\s]+)"[^}]*?"id"\s*:\s*(\d+)"#).unwrap()
    });
    for caps in re2.captures_iter(html) {
        let link = caps.get(1).unwrap().as_str().to_string();
        let id = caps.get(2).unwrap().as_str().to_string();
        map.entry(link).or_insert(id);
    }

    map
}

/// Seconds (Bandcamp sends floats like `273.5`) to whole ms. Zero, negative and non-finite values are "unknown".
fn secs_to_ms(secs: f64) -> Option<u64> {
    if secs.is_finite() && secs > 0.0 {
        Some((secs * 1000.0).round() as u64)
    } else {
        None
    }
}

/// The album/track data sits in a `data-tralbum="{&quot;...}"` attribute, HTML-escaped. Returns the page with
/// the quotes unescaped so the JSON-ish regexes below can read it (older pages carry it unescaped already).
fn unescape_tralbum(html: &str) -> std::borrow::Cow<'_, str> {
    if html.contains("&quot;") {
        std::borrow::Cow::Owned(html.replace("&quot;", "\"").replace("&amp;", "&"))
    } else {
        std::borrow::Cow::Borrowed(html)
    }
}

/// `/track/slug` -> duration in ms for every `trackinfo` entry that has both a `title_link` and a `duration`.
fn extract_album_track_durations_ms(html: &str) -> HashMap<String, u64> {
    static LINK_FIRST: OnceLock<Regex> = OnceLock::new();
    static DURATION_FIRST: OnceLock<Regex> = OnceLock::new();
    // `[^{}]*?` keeps a match inside one trackinfo object (the nested `file` object comes before both fields).
    let link_first = LINK_FIRST.get_or_init(|| {
        Regex::new(r#""title_link"\s*:\s*"(/track/[^"\s]+)"[^{}]*?"duration"\s*:\s*([0-9]+(?:\.[0-9]+)?)"#)
            .unwrap()
    });
    let duration_first = DURATION_FIRST.get_or_init(|| {
        Regex::new(r#""duration"\s*:\s*([0-9]+(?:\.[0-9]+)?)[^{}]*?"title_link"\s*:\s*"(/track/[^"\s]+)""#)
            .unwrap()
    });

    let text = unescape_tralbum(html);
    let mut map = HashMap::new();
    for caps in link_first.captures_iter(&text) {
        if let Some(ms) = caps[2].parse::<f64>().ok().and_then(secs_to_ms) {
            map.insert(caps[1].to_string(), ms);
        }
    }
    for caps in duration_first.captures_iter(&text) {
        if let Some(ms) = caps[1].parse::<f64>().ok().and_then(secs_to_ms) {
            map.entry(caps[2].to_string()).or_insert(ms);
        }
    }
    map
}

/// ISO-8601 duration as Bandcamp writes it in its JSON-LD (`P00H04M33S`) -> ms.
fn parse_iso_duration_ms(iso: &str) -> Option<u64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^P(?:T)?(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?$").unwrap()
    });
    let caps = re.captures(iso.trim())?;
    let part = |i: usize| -> f64 {
        caps.get(i)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    secs_to_ms(part(1) * 3600.0 + part(2) * 60.0 + part(3))
}

/// Duration of the track a track page is about: its `trackinfo` entry (matched by url when possible, else the
/// first one), falling back to the JSON-LD ISO duration.
fn extract_single_track_duration_ms(html: &str, track_url: &str) -> Option<u64> {
    let by_url = extract_album_track_durations_ms(html);
    if let Some(idx) = track_url.find("/track/") {
        let path = track_url[idx..].split(['?', '#']).next().unwrap_or("");
        if let Some(ms) = by_url.get(path.trim_end_matches('/')) {
            return Some(*ms);
        }
    }

    static NUMERIC_RE: OnceLock<Regex> = OnceLock::new();
    let numeric_re =
        NUMERIC_RE.get_or_init(|| Regex::new(r#""duration"\s*:\s*([0-9]+(?:\.[0-9]+)?)"#).unwrap());
    let text = unescape_tralbum(html);
    let from_trackinfo = text
        .find("\"trackinfo\"")
        .map(|start| &text[start..])
        .and_then(|rest| numeric_re.captures(rest))
        .and_then(|caps| caps[1].parse::<f64>().ok())
        .and_then(secs_to_ms);
    if from_trackinfo.is_some() {
        return from_trackinfo;
    }

    static ISO_RE: OnceLock<Regex> = OnceLock::new();
    let iso_re = ISO_RE.get_or_init(|| Regex::new(r#""duration"\s*:\s*"(P[^"]+)""#).unwrap());
    let json_ld = extract_json_ld_block(html)?;
    iso_re
        .captures(&json_ld)
        .and_then(|caps| parse_iso_duration_ms(&caps[1]))
}

#[cfg(test)]
mod tests {
    #[test]
    fn album_url_from_json_ld_in_album() {
        let html = r#"{"inAlbum":{"@type":"MusicAlbum","@id":"https://a.bandcamp.com/album/one","name":"One"}}"#;
        assert_eq!(
            extract_album_url(html, "https://a.bandcamp.com/track/x").as_deref(),
            Some("https://a.bandcamp.com/album/one")
        );
    }

    #[test]
    fn album_url_from_a_relative_link_and_none_for_singles() {
        let html = r#"<a href="/album/one">One</a>"#;
        assert_eq!(
            extract_album_url(html, "https://a.bandcamp.com/track/x").as_deref(),
            Some("https://a.bandcamp.com/album/one")
        );
        assert_eq!(extract_album_url("<p>none</p>", "https://a.bandcamp.com/track/x"), None);
    }

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
        assert_eq!(tracks[0].source_item_id, Some("in-absentia-2".to_string()));
        assert_eq!(
            tracks[1].source_item_id,
            Some("spiral-out-keep-going-2".to_string())
        );
    }

    #[test]
    fn test_extract_track_id_from_html_comment() {
        let html = "<div><!-- track id 3770803404 --></div>";
        assert_eq!(
            extract_track_id_from_html(html, None),
            Some("3770803404".to_string())
        );
    }

    #[test]
    fn test_extract_track_id_from_html_embed() {
        let html = r#"<meta property="og:video" content="https://bandcamp.com/EmbeddedPlayer/v=2/track=12345678/size=large/">"#;
        assert_eq!(
            extract_track_id_from_html(html, None),
            Some("12345678".to_string())
        );
    }

    #[test]
    fn test_extract_track_id_from_html_fallback_url() {
        let html = "<html></html>";
        assert_eq!(
            extract_track_id_from_html(
                html,
                Some("https://artist.bandcamp.com/track/my-track?action=download")
            ),
            Some("my-track".to_string())
        );
    }

    #[test]
    fn test_extract_album_track_ids() {
        let html = r#"
            var TralbumData = {
                trackinfo: [
                    { "id": 111, "title_link": "/track/first-song" },
                    { "id": 222, "title_link": "/track/second-song" }
                ]
            };
        "#;
        let map = extract_album_track_ids(html);
        assert_eq!(map.get("/track/first-song"), Some(&"111".to_string()));
        assert_eq!(map.get("/track/second-song"), Some(&"222".to_string()));
    }

    /// Shape of a real album page: escaped `data-tralbum` JSON with the nested `file` object before the
    /// fields we read.
    const ALBUM_HTML: &str = r#"
        <div id="pagedata" data-tralbum="{&quot;trackinfo&quot;:[
          {&quot;id&quot;:111,&quot;file&quot;:{&quot;mp3-128&quot;:&quot;https://t4.bcbits.com/stream/a&quot;},&quot;title&quot;:&quot;First Song&quot;,&quot;track_num&quot;:1,&quot;title_link&quot;:&quot;/track/first-song&quot;,&quot;has_lyrics&quot;:false,&quot;duration&quot;:273.5,&quot;lyrics&quot;:null},
          {&quot;id&quot;:222,&quot;file&quot;:{&quot;mp3-128&quot;:&quot;https://t4.bcbits.com/stream/b&quot;},&quot;title&quot;:&quot;Second Song&quot;,&quot;track_num&quot;:2,&quot;title_link&quot;:&quot;/track/second-song&quot;,&quot;duration&quot;:61.2345,&quot;lyrics&quot;:null},
          {&quot;id&quot;:333,&quot;file&quot;:null,&quot;title&quot;:&quot;Hidden&quot;,&quot;track_num&quot;:3,&quot;title_link&quot;:&quot;/track/hidden-song&quot;,&quot;duration&quot;:0,&quot;lyrics&quot;:null}
        ]}"></div>
        <a href="/track/first-song"><span class="track-title">First Song</span></a>
        <a href="/track/second-song"><span class="track-title">Second Song</span></a>
        <a href="/track/hidden-song"><span class="track-title">Hidden</span></a>
        <a href="/track/no-data"><span class="track-title">No Data</span></a>
    "#;

    #[test]
    fn secs_to_ms_rounds_and_rejects_unknown_values() {
        assert_eq!(secs_to_ms(273.5), Some(273_500));
        assert_eq!(secs_to_ms(61.2345), Some(61_235));
        assert_eq!(secs_to_ms(0.0), None, "zero means unknown, never 0");
        assert_eq!(secs_to_ms(-3.0), None);
        assert_eq!(secs_to_ms(f64::NAN), None);
        assert_eq!(secs_to_ms(f64::INFINITY), None);
    }

    #[test]
    fn album_durations_are_read_from_the_escaped_tralbum_data() {
        let map = extract_album_track_durations_ms(ALBUM_HTML);
        assert_eq!(map.get("/track/first-song"), Some(&273_500));
        assert_eq!(map.get("/track/second-song"), Some(&61_235));
        assert_eq!(map.get("/track/hidden-song"), None, "duration 0 is unknown");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn album_durations_also_work_on_unescaped_trackinfo() {
        let html = r#"var TralbumData = { trackinfo: [
            { "id": 1, "title_link": "/track/a", "duration": 10.5 },
            { "duration": 20, "id": 2, "title_link": "/track/b" } ] };"#;
        let map = extract_album_track_durations_ms(html);
        assert_eq!(map.get("/track/a"), Some(&10_500));
        assert_eq!(map.get("/track/b"), Some(&20_000));
    }

    #[test]
    fn album_without_durations_gives_an_empty_map() {
        assert!(extract_album_track_durations_ms("<html></html>").is_empty());
    }

    #[test]
    fn parse_tracks_fills_durations_and_leaves_unknown_ones_none() {
        let tracks = parse_tracks_from_html(ALBUM_HTML, "https://test.bandcamp.com").unwrap();
        assert_eq!(tracks.len(), 4);
        assert_eq!(tracks[0].duration_ms, Some(273_500));
        assert_eq!(tracks[1].duration_ms, Some(61_235));
        assert_eq!(tracks[2].duration_ms, None);
        assert_eq!(tracks[3].duration_ms, None);
    }

    #[test]
    fn iso_durations_parse() {
        assert_eq!(parse_iso_duration_ms("P00H04M33S"), Some(273_000));
        assert_eq!(parse_iso_duration_ms("PT1H02M03S"), Some(3_723_000));
        assert_eq!(parse_iso_duration_ms("P00H00M45.5S"), Some(45_500));
        assert_eq!(parse_iso_duration_ms("P00H00M00S"), None);
        assert_eq!(parse_iso_duration_ms("garbage"), None);
        assert_eq!(parse_iso_duration_ms(""), None);
    }

    #[test]
    fn single_track_duration_prefers_the_entry_of_that_url() {
        let ms = extract_single_track_duration_ms(ALBUM_HTML, "https://test.bandcamp.com/track/second-song?from=embed");
        assert_eq!(ms, Some(61_235));
    }

    #[test]
    fn single_track_duration_falls_back_to_the_first_trackinfo_entry() {
        let html = r#"<div data-tralbum="{&quot;trackinfo&quot;:[{&quot;title_link&quot;:null,&quot;duration&quot;:200.25}]}"></div>"#;
        assert_eq!(
            extract_single_track_duration_ms(html, "https://a.bandcamp.com/track/x"),
            Some(200_250)
        );
    }

    #[test]
    fn single_track_duration_falls_back_to_json_ld() {
        let html = r#"<script type="application/ld+json">{"@type":"MusicRecording","duration":"P00H03M05S"}</script>"#;
        assert_eq!(
            extract_single_track_duration_ms(html, "https://a.bandcamp.com/track/x"),
            Some(185_000)
        );
    }

    #[test]
    fn single_track_without_any_duration_is_none() {
        assert_eq!(extract_single_track_duration_ms("<html></html>", "https://a.bandcamp.com/track/x"), None);
    }
}
