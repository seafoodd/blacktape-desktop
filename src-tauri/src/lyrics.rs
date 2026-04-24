use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct LyricsResponse {
    lyrics: String,
}

#[derive(Deserialize)]
struct SuggestResponse {
    data: Vec<SongData>,
    total: usize,
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

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LyricsSource {
    pub lyrics: String,
    pub source: String,
}

pub async fn fetch_lyrics(artist_input: &str, title_input: &str) -> Result<LyricsSource, String> {
    let suggest_url = format!(
        "https://api.lyrics.ovh/suggest/{}-{}",
        urlencoding::encode(artist_input),
        urlencoding::encode(title_input)
    );

    let suggest_res = reqwest::get(&suggest_url)
        .await
        .map_err(|e| e.to_string())?;

    if !suggest_res.status().is_success() {
        return Err("Search failed".to_string());
    }

    let suggest_data: SuggestResponse = suggest_res.json().await.map_err(|e| e.to_string())?;

    let best_match = suggest_data
        .data
        .first()
        .ok_or_else(|| "No matching songs found".to_string())?;

    let clean_artist = &best_match.artist.name;
    let clean_title = &best_match.title_short;

    let lyrics_url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(clean_artist),
        urlencoding::encode(clean_title)
    );

    let lyrics_res = reqwest::get(&lyrics_url).await.map_err(|e| e.to_string())?;

    if lyrics_res.status().is_success() {
        let data: LyricsResponse = lyrics_res.json().await.map_err(|e| e.to_string())?;
        Ok(LyricsSource {
            lyrics: data.lyrics,
            source: "https://lyrics.ovh".to_string(),
        })
    } else {
        Err("Lyrics not found in the database".to_string())
    }
}
