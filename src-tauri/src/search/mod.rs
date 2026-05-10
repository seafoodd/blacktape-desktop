use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SearchSuggestion {
    pub item_type: String,
    pub name: String,
    pub band_name: String,
    pub album_name: Option<String>,
    pub item_url_path: String,
    pub img: String,
}

pub mod bandcamp;
