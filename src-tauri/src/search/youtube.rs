use crate::search::bandcamp::ItemType;
use crate::search::SearchSuggestion;
use crate::types::Platform;
use rustypipe::client::RustyPipe;
use rustypipe::model::MusicItem;
use rustypipe::param::search_filter::MusicSearchFilter;
use std::error::Error;
use tokio::try_join;

impl From<MusicItem> for SearchSuggestion {
    fn from(item: MusicItem) -> Self {
        match item {
            MusicItem::Track(t) => SearchSuggestion {
                item_type: ItemType::Track,
                name: t.name,
                band_name: t
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<String>>()
                    .join(", "),
                album_name: t.album.map(|a| a.name),
                item_url_path: format!("https://youtube.com/watch?v={}", t.id),
                img: t.cover.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: None,
                view_count: t.view_count,
                year: None,
                duration: t.duration,
                platform: Platform::Youtube,
            },
            MusicItem::Artist(a) => SearchSuggestion {
                item_type: ItemType::Artist,
                name: a.name,
                band_name: "Artist".to_string(),
                album_name: None,
                item_url_path: format!("https://youtube.com/browse/{}", a.id),
                img: a.avatar.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: a.subscriber_count,
                view_count: None,
                year: None,
                duration: None,
                platform: Platform::Youtube,
            },
            MusicItem::Album(a) => SearchSuggestion {
                item_type: ItemType::Album,
                name: a.name,
                band_name: a
                    .artists
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<String>>()
                    .join(", "),
                album_name: None,
                item_url_path: format!("https://music.youtube.com/browse/{}", a.id),
                img: a.cover.last().map(|c| c.url.clone()).unwrap_or_default(),
                subscriber_count: None,
                view_count: None,
                year: a.year.map(|y| y as i32),
                duration: None,
                platform: Platform::Youtube,
            },
            _ => SearchSuggestion {
                item_type: ItemType::Unknown,
                name: String::new(),
                band_name: String::new(),
                album_name: None,
                item_url_path: String::new(),
                img: String::new(),
                subscriber_count: None,
                view_count: None,
                year: None,
                duration: None,
                platform: Platform::Youtube,
            },
        }
    }
}

pub async fn search(query: &str) -> Result<Vec<SearchSuggestion>, Box<dyn Error + Send + Sync>> {
    let rp = RustyPipe::new();

    let track_query = rp.query();
    let album_query = rp.query();
    let artist_query = rp.query();

    let (track_res, album_res, artist_res) = try_join!(
        track_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Tracks)),
        album_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Albums)),
        artist_query.music_search::<MusicItem, _>(query, Some(MusicSearchFilter::Artists))
    )?;

    let mut items = Vec::new();
    items.extend(
        track_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from),
    );
    items.extend(
        album_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from),
    );
    items.extend(
        artist_res
            .items
            .items
            .into_iter()
            .map(SearchSuggestion::from),
    );

    Ok(items)
}
