use crate::types::Platform;
pub(crate) mod http;

pub async fn search_dropdown(platform: Platform, query: &str) -> Result<crate::search::SourceResults, String> {
    use crate::search::{self, SourceResults};
    match platform {
        Platform::Youtube => search::search_youtube_with_fallback(query).await.map(SourceResults::complete).map_err(|error| error.to_string()),
        Platform::Bandcamp => search::bandcamp::search(query).await.map(SourceResults::complete).map_err(|error| error.to_string()),
        Platform::Soundcloud => search::soundcloud::search_outcome(query).await.map_err(|error| error.to_string()),
        Platform::Local => Err("Local files cannot be searched remotely".into()),
    }
}

pub(crate) async fn search_page(platform: Platform, query: &str, kind: crate::search::full::SearchKind, cursor: Option<&crate::search::full::SearchCursor>) -> Result<crate::search::full::Fetched, String> {
    use crate::search::full;
    match platform {
        Platform::Youtube => match cursor.and_then(|cursor| cursor.youtube.as_deref()) {
            Some(token) => full::youtube_more(query, kind, token).await,
            None => full::youtube_first_page(query, kind).await,
        },
        Platform::Bandcamp => full::bandcamp_page(query, kind).await,
        Platform::Soundcloud => full::soundcloud_page(query, kind, cursor.and_then(|cursor| cursor.soundcloud).unwrap_or(0)).await,
        Platform::Local => Err("Local files have no remote search pages".into()),
    }
}

pub async fn artist_profile(platform: Platform, url: &str) -> Result<crate::search::full::ArtistProfile, String> {
    use crate::search;
    match platform {
        Platform::Youtube => search::youtube_full::artist_profile(url).await.map_err(|error| error.to_string()),
        Platform::Bandcamp => search::bandcamp_full::artist_profile(url).await.map_err(|error| error.to_string()),
        Platform::Soundcloud => search::soundcloud::artist_profile(url).await,
        Platform::Local => Err("artist profiles are not available for local files".into()),
    }
}

pub async fn album_metadata(platform: Platform, url: &str) -> Result<(crate::download::AlbumDownload, Vec<crate::download::soundcloud::SkippedTrack>, bool), String> {
    use crate::download;
    match platform {
        Platform::Youtube => download::youtube::parse_album(url).await.map(|album| (album, Vec::new(), false)),
        Platform::Bandcamp => download::bandcamp::parse_album_with_free_flag(url).await.map(|(album, free)| (album, Vec::new(), free)),
        Platform::Soundcloud => download::soundcloud::parse_album_with_skipped(url).await.map(|(album, skipped)| (album, skipped, false)),
        Platform::Local => Err("Local albums have no remote metadata".into()),
    }
}

pub async fn track_candidates(platform: Platform, artist: &str, title: &str, duration_ms: u64) -> Result<Vec<crate::metadata::types::AudioCandidate>, String> {
    use crate::resolver::provider;
    match platform {
        Platform::Youtube => provider::resolve_youtube_candidates(artist, title, duration_ms).await,
        Platform::Bandcamp => provider::resolve_bandcamp_candidates(artist, title, duration_ms).await,
        Platform::Soundcloud => provider::resolve_soundcloud_candidates(artist, title, duration_ms).await,
        Platform::Local => Err("Local tracks have no remote candidates".into()),
    }
}

pub async fn album_tracks(platform: Platform, artist: &str, title: &str) -> Result<(Vec<crate::download::TrackDownload>, bool), String> {
    use crate::resolver::provider;
    match platform {
        Platform::Youtube => provider::resolve_youtube_album_tracks(artist, title).await.map(|tracks| (tracks, false)),
        Platform::Bandcamp => provider::resolve_bandcamp_album_tracks(artist, title).await,
        Platform::Soundcloud => provider::resolve_soundcloud_album_tracks(artist, title).await.map(|tracks| (tracks, false)),
        Platform::Local => Err("Local albums have no remote tracks".into()),
    }
}

pub async fn stream(app: &tauri::AppHandle, platform: Platform, url: &str) -> Result<crate::preview::resolve::StreamInfo, String> {
    use crate::preview::resolve;
    match platform {
        Platform::Youtube => resolve::resolve_youtube(app, url).await,
        Platform::Bandcamp => resolve::resolve_bandcamp(url).await,
        Platform::Soundcloud => resolve::resolve_soundcloud(url).await,
        Platform::Local => Err("local files are already playable".into()),
    }
}

#[cfg(test)]
mod dispatch_tests;

/// Classifies a page by its HTTP(S) host, never by text in its path or query.
pub fn platform_of_url(raw: &str) -> Option<Platform> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.trim_end_matches('.');
    let on_domain = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on_domain("youtube.com") || on_domain("youtu.be") {
        Some(Platform::Youtube)
    } else if on_domain("bandcamp.com") {
        Some(Platform::Bandcamp)
    } else if on_domain("soundcloud.com") || on_domain("snd.sc") {
        Some(Platform::Soundcloud)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
