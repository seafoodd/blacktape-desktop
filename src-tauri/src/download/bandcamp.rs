use crate::download::{AlbumDownload, TrackDownload};
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

    let tracks = parse_tracks_from_html(&html, base_url)?;

    Ok(AlbumDownload {
        title: "".to_string(),
        artists: vec![],
        tracks,
        genres: None,
        release_year: None,
        external_cover_url: None,
    })
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
            album: "".to_string(),
            track_number: None,
            genres: None,
            release_year: None,
        });

        track_count += 1;
    }

    if tracks.is_empty() {
        return Err("Could not find any tracks inside the target HTML layout".to_string());
    }

    Ok(tracks)
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
