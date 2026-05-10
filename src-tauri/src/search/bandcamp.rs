use crate::search::SearchSuggestion;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct SearchRequest {
    search_text: String,
    search_filter: String,
    full_page: bool,
}

#[derive(Debug, Deserialize)]
pub struct BandcampSearchResponse {
    pub auto: AutoSection,
}

#[derive(Debug, Deserialize)]
pub struct AutoSection {
    pub results: Vec<BandcampResult>,
}

#[derive(Debug, Deserialize)]
pub struct BandcampResult {
    #[serde(rename = "type")]
    pub item_type: String,
    pub id: u64,
    pub name: String,
    pub band_name: Option<String>,
    pub img: String,
    pub item_url_path: Option<String>,
    pub album_name: Option<String>,
    pub album_id: Option<u64>,
}

impl From<BandcampResult> for SearchSuggestion {
    fn from(res: BandcampResult) -> Self {
        let display_artist = res.band_name.unwrap_or_else(|| res.name.clone());

        Self {
            item_type: res.item_type,
            name: res.name,
            band_name: display_artist,
            album_name: res.album_name,
            item_url_path: res.item_url_path.unwrap_or_default(),
            img: res.img.replace("/img/", "/img/a"),
        }
    }
}

pub async fn search(query: String) -> Result<Vec<SearchSuggestion>, Box<dyn std::error::Error>> {
    let client = Client::new();
    let url = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";

    let request_body = SearchRequest {
        search_text: query,
        search_filter: String::new(),
        full_page: false,
    };

    let response = client.post(url).json(&request_body).send().await?;

    if response.status().is_success() {
        let body_text = response.text().await?;
        let parsed: BandcampSearchResponse = serde_json::from_str(&body_text)
            .map_err(|e| format!("JSON Error: {} | Body: {}", e, body_text))?;
        let suggestions = parsed
            .auto
            .results
            .into_iter()
            .map(SearchSuggestion::from)
            .collect();
        Ok(suggestions)
    } else {
        Err(format!("Bandcamp API error: {}", response.status()).into())
    }
}
