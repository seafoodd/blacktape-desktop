//! lyrics.ovh provider — plain (unsynced) lyrics only.

use super::common::{get_json, is_acceptable, track_matches, LyricsSource};
use serde::Deserialize;

const SOURCE: &str = "https://lyrics.ovh";

#[derive(Deserialize)]
struct LyricsResponse {
    lyrics: String,
}

#[derive(Deserialize)]
struct SuggestResponse {
    data: Vec<SongData>,
}

#[derive(Deserialize)]
struct SongData {
    title_short: String,
    artist: ArtistData,
}

#[derive(Deserialize)]
struct ArtistData {
    name: String,
}

async fn get_lyrics(client: &reqwest::Client, artist: &str, title: &str) -> Option<LyricsSource> {
    // `percent_encoding` escapes '/', so inputs cannot alter the URL path.
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        percent_encoding::utf8_percent_encode(artist, percent_encoding::NON_ALPHANUMERIC),
        percent_encoding::utf8_percent_encode(title, percent_encoding::NON_ALPHANUMERIC)
    );
    let data: LyricsResponse = get_json(client, &url, "Blacktape").await.ok()?;
    is_acceptable(&data.lyrics).then(|| LyricsSource {
        lyrics: data.lyrics,
        source: SOURCE.to_string(),
    })
}

pub async fn fetch(
    client: &reqwest::Client,
    artist_input: &str,
    title_input: &str,
) -> Result<LyricsSource, String> {
    let suggest_url = format!(
        "https://api.lyrics.ovh/suggest/{}-{}",
        percent_encoding::utf8_percent_encode(artist_input, percent_encoding::NON_ALPHANUMERIC),
        percent_encoding::utf8_percent_encode(title_input, percent_encoding::NON_ALPHANUMERIC)
    );

    let mut clean_artist = artist_input.to_string();
    let mut clean_title = title_input.to_string();
    if let Ok(s) = get_json::<SuggestResponse>(client, &suggest_url, "Blacktape").await {
        // Only trust a suggestion that is really the requested song.
        if let Some(best) = s
            .data
            .iter()
            .find(|d| track_matches(artist_input, title_input, &d.artist.name, &d.title_short))
        {
            clean_artist = best.artist.name.clone();
            clean_title = best.title_short.clone();
        }
    }

    if let Some(r) = get_lyrics(client, &clean_artist, &clean_title).await {
        return Ok(r);
    }
    if clean_artist != artist_input || clean_title != title_input {
        if let Some(r) = get_lyrics(client, artist_input, title_input).await {
            return Ok(r);
        }
    }
    Err("lyrics.ovh: no lyrics found".to_string())
}
