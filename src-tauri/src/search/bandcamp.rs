use crate::search::SearchSuggestion;
use crate::types::Platform;
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
    pub results: Vec<serde_json::Value>,
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
    pub img: Option<String>,
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

        let image = res.img.unwrap_or_default();
        let img_src = match item_type {
            ItemType::Album => image.replace("/img/", "/img/a"),
            ItemType::Track => image.replace("/img/", "/img/a"),

            _ => image,
        };

        Self {
            item_type,
            name: res.name,
            band_name: display_artist,
            album_name: res.album_name,
            subscriber_count: None,
            audience_kind: None,
            album_url: None,
            view_count: None,
            year: None,
            item_url_path: res.item_url_path.unwrap_or_default(),
            img: img_src,
            duration: None,
            platform: Platform::Bandcamp,
        }
    }
}

fn parse_response(body: &str) -> Result<Vec<SearchSuggestion>, String> {
    let parsed: BandcampSearchResponse = serde_json::from_str(body)
        .map_err(|_| "Bandcamp returned an invalid search response".to_string())?;
    Ok(parsed.auto.results.into_iter()
        .filter_map(|row| serde_json::from_value::<BandcampResult>(row).ok())
        .map(SearchSuggestion::from)
        .collect())
}

#[cfg(test)]
#[path = "bandcamp_tests.rs"]
mod tests;

pub async fn search(query: &str) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let url = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";

    let request_body = SearchRequest {
        search_text: query.to_string(),
        search_filter: String::new(),
        full_page: false,
    };

    let body = crate::platforms::http::post_page_json(url, &request_body).await?;
    parse_response(&body).map_err(Into::into)
}
