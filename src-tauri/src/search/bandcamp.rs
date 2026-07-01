use crate::search::SearchSuggestion;
use crate::types::Platform;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

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

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub enum ItemType {
    Album,
    Artist,
    Track,
    Unknown,
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
        let item_type = match res.item_type.as_str() {
            "a" => ItemType::Album,
            "t" => ItemType::Track,
            "b" => ItemType::Artist,
            _ => ItemType::Unknown,
        };

        let display_artist = res.band_name.unwrap_or_else(|| res.name.clone());

        let img_src = match item_type {
            ItemType::Album => res.img.replace("/img/", "/img/a"),
            ItemType::Track => res.img.replace("/img/", "/img/a"),

            _ => res.img,
        };

        Self {
            item_type,
            name: res.name,
            band_name: display_artist,
            album_name: res.album_name,
            subscriber_count: None,
            view_count: None,
            year: None,
            item_url_path: res.item_url_path.unwrap_or_default(),
            img: img_src,
            duration: None,
            platform: Platform::Bandcamp,
        }
    }
}

pub async fn search(query: &str) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let client = Client::new();
    let url = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";

    let request_body = SearchRequest {
        search_text: query.to_string(),
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
