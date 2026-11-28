use crate::platforms::musicbrainz::MusicBrainzClient;
use crate::metadata::types::{CanonicalRelease, CanonicalReleaseSummary, CanonicalTrack, ReleaseEditionOption};
use crate::types::Platform;
use tauri::{command, AppHandle, Manager};

/// Searches MusicBrainz for canonical releases matching `query`.
#[command]
pub async fn search_canonical_releases(
    app: AppHandle,
    query: String,
) -> Result<Vec<CanonicalReleaseSummary>, String> {
    let cache_dir = app.path().app_cache_dir().ok();
    let client = MusicBrainzClient::new(cache_dir);
    let raw_summaries = client.search_release_groups(&query).await?;
    Ok(raw_summaries
        .into_iter()
        .map(|s| s.into_canonical_summary())
        .collect())
}

/// Fetches a full canonical release with complete tracklist and disc layout.
#[command]
pub async fn get_canonical_release(
    app: AppHandle,
    id: String,
) -> Result<CanonicalRelease, String> {
    let cache_dir = app.path().app_cache_dir().ok();
    let client = MusicBrainzClient::new(cache_dir);
    client.fetch_canonical_release(&id).await
}

/// Fetches deduplicated distinct release editions for a MusicBrainz release-group,
/// plus matching platform album releases (YouTube Music, Bandcamp, SoundCloud).
#[command]
pub async fn get_release_group_releases(
    app: AppHandle,
    rg_id: Option<String>,
    artist: Option<String>,
    title: Option<String>,
) -> Result<Vec<ReleaseEditionOption>, String> {
    let cache_dir = app.path().app_cache_dir().ok();
    let client = MusicBrainzClient::new(cache_dir);
    let clean_rg = rg_id.unwrap_or_default().trim().to_string();

    let mb_fut = async {
        if !clean_rg.is_empty() {
            client.list_grouped_editions_for_group(&clean_rg).await
        } else if let (Some(a), Some(t)) = (artist.as_deref(), title.as_deref()) {
            if !a.trim().is_empty() && !t.trim().is_empty() {
                if let Some(matched_rg) = client.find_matching_release_group_id(a, t).await {
                    Ok(client
                        .list_grouped_editions_for_group(&matched_rg)
                        .await
                        .unwrap_or_default())
                } else {
                    Ok(Vec::new())
                }
            } else {
                Ok(Vec::new())
            }
        } else {
            Ok(Vec::new())
        }
    };
    let plat_fut = async {
        match (artist.as_deref(), title.as_deref()) {
            (Some(a), Some(t)) if !a.trim().is_empty() && !t.trim().is_empty() => {
                crate::resolver::provider::find_platform_album_editions(a, t).await
            }
            _ => Vec::new(),
        }
    };

    let (mb_res, platform_editions) = tokio::join!(mb_fut, plat_fut);
    let mut editions = if clean_rg.is_empty() {
        mb_res.unwrap_or_default()
    } else {
        mb_res?
    };
    editions.extend(platform_editions);
    Ok(editions)
}

/// Resolves audio candidates for a single canonical track across all platforms.
#[command]
pub async fn resolve_canonical_candidates(
    mut track: CanonicalTrack,
    fallback_artist: Option<String>,
) -> Result<CanonicalTrack, String> {
    crate::resolver::populate_track_candidates(&mut track, fallback_artist.as_deref()).await;
    Ok(track)
}

/// Prewarms stream resolution for tracks in the background to achieve near-instant preview playback.
#[command]
pub async fn prewarm_album_previews(
    app: AppHandle,
    items: Vec<(Platform, String)>,
) -> Result<(), String> {
    for (platform, url) in items {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let _ = crate::preview::resolve::resolve(&app_clone, platform, &url).await;
        });
    }
    Ok(())
}
