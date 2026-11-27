use super::youtube;
use super::{bandcamp, soundcloud, ytdlp};
use crate::db::db::Database;
use crate::download::soundcloud::SkippedTrack;
use crate::download::types::{
    AlbumDownload, AlbumMetaEvent, DownloadResult, ProgressEvent, TrackDownload, TrackMetaItem,
};
use crate::library_index::sources::build_album_source;
use crate::types::Platform;
use crate::utils::{remove_empty_parents_up_to, sanitize_fs};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// Keeps only the tracks whose url is in `selected`, in album order. Pure.
#[cfg(test)]
fn select_tracks(tracks: Vec<TrackDownload>, selected: &[String]) -> Vec<TrackDownload> {
    let positions = selected_track_positions(&tracks, &[], selected);
    tracks.into_iter().enumerate().filter_map(|(index, track)| positions.contains(&index).then_some(track)).collect()
}

fn selected_track_positions(
    tracks: &[TrackDownload], canonical_urls: &[String], selected: &[String],
) -> std::collections::HashSet<usize> {
    tracks.iter().enumerate().filter_map(|(index, track)| {
        let original = canonical_urls.get(index).map_or(track.url.as_str(), String::as_str);
        selected.iter().any(|url| !url.is_empty() && same_page(url, original)).then_some(index)
    }).collect()
}

fn same_page(a: &str, b: &str) -> bool {
    use crate::library_index::normalize::normalize_url_key;
    a == b || matches!((normalize_url_key(a), normalize_url_key(b)), (Some(x), Some(y)) if x == y)
}

/// The tracks that are not in the library yet (by page url, artist + title or a near title), in album order.
/// Pure: `index` is the library.
pub fn without_owned(
    platform: Platform,
    album_artist: &str,
    tracks: Vec<TrackDownload>,
    index: &crate::library_index::status::LibraryIndex,
) -> Vec<TrackDownload> {
    use crate::download::album_preview::AlbumTrackInfo;
    let infos: Vec<AlbumTrackInfo> = tracks
        .iter()
        .enumerate()
        .map(|(index, t)| AlbumTrackInfo {
            index,
            title: t.title.clone(),
            artists: t.artists.clone(),
            track_number: t.track_number,
            disc_number: t.disc_number,
            url: t.url.clone(),
            available: true,
            note: None,
            duration_ms: None,
            candidates: Vec::new(),
            selected_candidate_idx: 0,
        })
        .collect();
    let missing: Vec<String> = crate::library_index::sources::compute_missing(platform, album_artist, &infos, index)
        .into_iter()
        .map(|t| t.url)
        .collect();
    tracks.into_iter().filter(|t| missing.contains(&t.url)).collect()
}

/// Builds the library index (songs of the library) from the database.
async fn library_index(
    app_handle: &AppHandle,
    album_artist: &str,
    album: &str,
) -> Option<crate::library_index::status::LibraryIndex> {
    let db_state = app_handle.try_state::<tokio::sync::Mutex<Database>>()?;
    let db = db_state.lock().await;
    let refs = db.list_song_source_refs().await.ok()?;
    let meta = db.list_song_meta().await.ok()?;
    // names are compared with the songs of this album only (another album may have a song of the same name)
    Some(crate::library_index::commands::scoped_index(
        &crate::library_index::status::LibraryIndex::from_rows(&refs),
        &meta,
        album_artist,
        album,
    ))
}

/// Stores the whole track list of the album in `album_sources` (library-sync D5) so the library can tell
/// partial from complete albums and list the missing songs. Returns whether a row was written. The key and the
/// stored `album` / `album_artist` are what `post_processor` writes into `songs.album` / `songs.album_artist`.
pub async fn write_album_source(
    db: &Database,
    platform: Platform,
    url: &str,
    album: &AlbumDownload,
    skipped: &[SkippedTrack],
    now: i64,
) -> Result<bool, sqlx::Error> {
    match build_album_source(platform, url, album, skipped, now) {
        Some(source) => {
            // the album was just downloaded from this platform album: an older link to another one goes
            db.replace_album_source(&source).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Best effort: a failure here is only logged, the download must not depend on the bookkeeping.
async fn record_album_source(
    app_handle: &AppHandle,
    platform: Platform,
    url: &str,
    album: &AlbumDownload,
    skipped: &[SkippedTrack],
) {
    let Some(db_state) = app_handle.try_state::<tokio::sync::Mutex<Database>>() else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let db = db_state.lock().await;
    if let Err(e) = write_album_source(&db, platform, url, album, skipped, now).await {
        tracing::error!(url = %url, error = %e, "Could not store album source");
    }
}

pub fn detect_platform_from_url(url: &str, fallback: Platform) -> Platform {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("magnet:") || lower.starts_with("torrent:") {
        Platform::Local
    } else if lower.contains("bandcamp.com") {
        Platform::Bandcamp
    } else if lower.contains("soundcloud.com") {
        Platform::Soundcloud
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        Platform::Youtube
    } else {
        fallback
    }
}

fn local_tier_rank(tier: crate::types::QualityTier) -> u8 {
    match tier {
        crate::types::QualityTier::Low => 0,
        crate::types::QualityTier::Standard => 1,
        crate::types::QualityTier::High => 2,
        crate::types::QualityTier::Lossless => 3,
    }
}

fn candidate_tier_rank(tier: crate::metadata::types::AudioQualityTier) -> u8 {
    match tier {
        crate::metadata::types::AudioQualityTier::Low => 0,
        crate::metadata::types::AudioQualityTier::StandardLossy => 1,
        crate::metadata::types::AudioQualityTier::HighLossy => 2,
        crate::metadata::types::AudioQualityTier::Lossless => 3,
    }
}

/// True when downloading `cand_tier` would produce strictly better quality than `local_tier`,
/// or when the local audio file is missing on disk. Pure.
pub fn candidate_improves_local(
    local_tier: crate::types::QualityTier,
    local_file_exists: bool,
    cand_tier: crate::metadata::types::AudioQualityTier,
) -> bool {
    !local_file_exists || candidate_tier_rank(cand_tier) > local_tier_rank(local_tier)
}

fn quality_tier_slug(tier: crate::types::QualityTier) -> &'static str {
    match tier {
        crate::types::QualityTier::Low => "low",
        crate::types::QualityTier::Standard => "standard",
        crate::types::QualityTier::High => "high",
        crate::types::QualityTier::Lossless => "lossless",
    }
}

/// Selects the matching candidate index along with its primary URL and fallback URLs. Pure.
pub fn pick_candidate_entry_for_tier(
    candidates: &[crate::metadata::types::AudioCandidate],
    preferred_format: Option<&str>,
    selected_idx: usize,
) -> Option<(usize, String, Vec<String>)> {
    use crate::metadata::types::AudioQualityTier;
    if candidates.is_empty() {
        return None;
    }

    let fmt = preferred_format.map(str::trim).unwrap_or("");
    let chosen_pos = if let Some(source_spec) = fmt.strip_prefix("source:") {
        let mut parts = source_spec.splitn(3, ':');
        let target_plat = parts.next().unwrap_or("").to_ascii_lowercase();
        let target_tier = parts.next().unwrap_or("").to_ascii_lowercase();
        let target_hash = parts.next().filter(|s| !s.is_empty());
        let matches_plat_tier = |c: &crate::metadata::types::AudioCandidate| {
            let plat_ok = c.source.platform_name().eq_ignore_ascii_case(&target_plat);
            if !plat_ok {
                return false;
            }
            match target_tier.as_str() {
                "standard" => c.quality_tier == AudioQualityTier::StandardLossy,
                "high" => c.quality_tier == AudioQualityTier::HighLossy,
                "lossless" => c.quality_tier == AudioQualityTier::Lossless,
                _ => true,
            }
        };
        target_hash
            .and_then(|hash| {
                candidates.iter().position(|c| match &c.source {
                    crate::metadata::types::AudioSource::Torrent { info_hash, .. } => {
                        matches_plat_tier(c) && info_hash.eq_ignore_ascii_case(hash)
                    }
                    _ => false,
                })
            })
            .or_else(|| candidates.iter().position(matches_plat_tier))
            .or_else(|| {
                candidates
                    .iter()
                    .position(|c| c.source.platform_name().eq_ignore_ascii_case(&target_plat))
            })
            .unwrap_or(0)
    } else if fmt.eq_ignore_ascii_case("standard") {
        candidates
            .iter()
            .position(|c| c.quality_tier == AudioQualityTier::StandardLossy)
            .or_else(|| candidates.iter().position(|c| c.quality_tier == AudioQualityTier::Low))
            .unwrap_or(0)
    } else if fmt.eq_ignore_ascii_case("high") {
        candidates
            .iter()
            .position(|c| c.quality_tier == AudioQualityTier::HighLossy)
            .unwrap_or(0)
    } else if selected_idx < candidates.len() {
        selected_idx
    } else {
        0
    };

    let primary = candidates[chosen_pos].source.primary_url().to_string();
    let mut fallbacks = Vec::new();
    for (idx, c) in candidates.iter().enumerate() {
        if idx == chosen_pos {
            continue;
        }
        let u = c.source.primary_url().to_string();
        if !u.is_empty() && u != primary && !fallbacks.contains(&u) {
            fallbacks.push(u);
        }
    }
    Some((chosen_pos, primary, fallbacks))
}

pub fn pick_candidate_urls_for_tier(
    candidates: &[crate::metadata::types::AudioCandidate],
    preferred_format: Option<&str>,
    selected_idx: usize,
) -> (String, Vec<String>) {
    match pick_candidate_entry_for_tier(candidates, preferred_format, selected_idx) {
        Some((_, primary, fallbacks)) => (primary, fallbacks),
        None => (String::new(), Vec::new()),
    }
}

pub(crate) fn apply_candidate_to_track(
    track: &mut TrackDownload,
    candidates: &[crate::metadata::types::AudioCandidate],
    preferred_format: Option<&str>,
    selected_idx: usize,
) -> Option<crate::metadata::types::AudioQualityTier> {
    use crate::metadata::types::AudioSource;

    let (chosen_idx, _, _) = pick_candidate_entry_for_tier(candidates, preferred_format, selected_idx)?;
    let chosen = &candidates[chosen_idx];
    let http_urls: Vec<String> = candidates
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx != chosen_idx)
        .filter_map(|(_, c)| match &c.source {
            AudioSource::HttpStream { url, .. } if !url.trim().is_empty() => Some(url.clone()),
            _ => None,
        })
        .collect();

    match &chosen.source {
        AudioSource::Torrent {
            info_hash,
            magnet_uri,
            file_index,
            file_path,
            file_size,
            ..
        } => {
            track.torrent_target = Some(crate::download::types::TorrentTarget {
                info_hash: info_hash.clone(),
                magnet_uri: magnet_uri.clone(),
                file_index: *file_index,
                file_path: file_path.clone(),
                file_size: *file_size,
                quality_tier: chosen.quality_tier,
                alternatives: torrent_alternatives(candidates, chosen_idx),
            });
            // Keep HTTP identity URL if present while storing HTTP fallbacks.
            let mut fallbacks = Vec::new();
            if track.url.starts_with("http://") || track.url.starts_with("https://") {
                fallbacks.push(track.url.clone());
            } else if let Some(first_http) = http_urls.first() {
                track.url = first_http.clone();
            } else {
                track.url = magnet_uri.clone();
            }
            for u in http_urls {
                if !fallbacks.contains(&u) {
                    fallbacks.push(u);
                }
            }
            track.fallback_urls = fallbacks;
        }
        AudioSource::HttpStream { url, .. } => {
            track.torrent_target = None;
            if !url.trim().is_empty() {
                track.url = url.clone();
            }
            let mut fallbacks = Vec::new();
            for u in http_urls {
                if u != track.url && !fallbacks.contains(&u) {
                    fallbacks.push(u);
                }
            }
            track.fallback_urls = fallbacks;
        }
        AudioSource::LocalFile { .. } => {
            track.torrent_target = None;
        }
    }

    Some(chosen.quality_tier)
}

fn torrent_alternatives(
    candidates: &[crate::metadata::types::AudioCandidate],
    chosen_idx: usize,
) -> Vec<crate::download::types::TorrentTarget> {
    use crate::metadata::types::AudioSource;
    let chosen = &candidates[chosen_idx];
    let AudioSource::Torrent { info_hash: chosen_hash, .. } = &chosen.source else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::from([chosen_hash.as_str()]);
    candidates.iter().filter_map(|candidate| {
        let AudioSource::Torrent { info_hash, magnet_uri, file_index, file_path, file_size, .. } = &candidate.source else {
            return None;
        };
        if candidate.quality_tier != chosen.quality_tier || !seen.insert(info_hash) {
            return None;
        }
        Some(crate::download::types::TorrentTarget {
            info_hash: info_hash.clone(),
            magnet_uri: magnet_uri.clone(),
            file_index: *file_index,
            file_path: file_path.clone(),
            file_size: *file_size,
            quality_tier: candidate.quality_tier,
            alternatives: Vec::new(),
        })
    }).take(5).collect()
}

pub fn apply_download_overrides(
    album_info: &mut AlbumDownload,
    overrides: &crate::download::types::DownloadOverrides,
) {
    apply_download_overrides_with_format(album_info, overrides, None);
}

pub fn apply_download_overrides_with_format(
    album_info: &mut AlbumDownload,
    overrides: &crate::download::types::DownloadOverrides,
    effective_format: Option<&str>,
) {
    if let Some(ref title) = overrides.title {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            album_info.title = trimmed.to_string();
        }
    }
    if let Some(ref artist) = overrides.artist {
        let trimmed = artist.trim();
        if !trimmed.is_empty() {
            album_info.album_artist = trimmed.to_string();
            album_info.artists = vec![trimmed.to_string()];
        }
    }
    if let Some(year) = overrides.year {
        album_info.release_year = Some(year);
    }

    for track in &mut album_info.tracks {
        track.album = album_info.title.clone();
        track.album_artist = album_info.album_artist.clone();
        if overrides.year.is_some() {
            track.release_year = album_info.release_year;
        }
    }

    if let Some(ref track_overrides) = overrides.tracks {
        for ov in track_overrides {
            let index = match &ov.original_url {
                Some(url) => ov.index
                    .filter(|&index| album_info.tracks.get(index).is_some_and(|track| same_page(&track.url, url)))
                    .or_else(|| album_info.tracks.iter().position(|track| same_page(&track.url, url))),
                None => ov.index,
            };
            let Some(idx) = index else {
                continue;
            };
            let Some(track) = album_info.tracks.get_mut(idx) else {
                continue;
            };
            if let Some(ref title) = ov.title {
                let trimmed = title.trim();
                if !trimmed.is_empty() {
                    track.title = trimmed.to_string();
                }
            }
            if let Some(ref url) = ov.url {
                let trimmed = url.trim();
                if !trimmed.is_empty() {
                    track.url = trimmed.to_string();
                }
            }
            if let Some(ref fallbacks) = ov.fallback_urls {
                track.fallback_urls = fallbacks.clone();
            }
            if let Some(ref cands) = ov.candidates {
                if !cands.is_empty() {
                    apply_candidate_to_track(
                        track,
                        cands,
                        effective_format,
                        ov.selected_candidate_idx.unwrap_or(0),
                    );
                }
            }
        }
    }
}

type UpgradeCacheMap = std::collections::HashMap<Vec<i64>, (Vec<Vec<crate::metadata::types::AudioCandidate>>, std::time::Instant)>;

fn upgrade_candidate_cache() -> &'static std::sync::Mutex<UpgradeCacheMap> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<UpgradeCacheMap>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn load_ordered_songs(
    app_handle: &AppHandle,
    song_ids: &[i64],
) -> Result<Vec<crate::Song>, String> {
    if song_ids.is_empty() {
        return Err("No library tracks specified for bulk upgrade.".to_string());
    }
    let db_state = app_handle
        .try_state::<tokio::sync::Mutex<Database>>()
        .ok_or_else(|| "Database state unavailable for album upgrade".to_string())?;
    let fetched = {
        let db = db_state.lock().await;
        db.get_songs_by_ids(song_ids)
            .await
            .map_err(|e| format!("Failed reading tracks for upgrade: {e}"))?
    };

    let by_id: HashMap<i64, crate::Song> = fetched
        .into_iter()
        .filter_map(|s| s.id.map(|id| (id, s)))
        .collect();
    let mut ordered_songs: Vec<crate::Song> = Vec::with_capacity(song_ids.len());
    for id in song_ids {
        if let Some(s) = by_id.get(id) {
            ordered_songs.push(s.clone());
        }
    }
    if ordered_songs.is_empty() {
        return Err("None of the selected tracks exist in the library anymore.".to_string());
    }
    Ok(ordered_songs)
}

async fn resolve_upgrade_candidates_cached(
    ordered_songs: &[crate::Song],
) -> Vec<Vec<crate::metadata::types::AudioCandidate>> {
    use crate::metadata::types::AudioCandidate;
    use crate::resolver::arbiter;
    use std::collections::HashSet;

    let cache_key: Vec<i64> = ordered_songs.iter().filter_map(|s| s.id).collect();
    if !cache_key.is_empty() {
        let guard = upgrade_candidate_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, at)) = guard.get(&cache_key) {
            if at.elapsed() < std::time::Duration::from_secs(300) && cached.len() == ordered_songs.len() {
                return cached.clone();
            }
        }
    }

    let first = &ordered_songs[0];
    let distinct_albums: HashSet<(String, String)> = ordered_songs
        .iter()
        .map(|s| (s.album_artist.to_lowercase(), s.album.to_lowercase()))
        .collect();
    let album_title = if distinct_albums.len() == 1 && !first.album.is_empty() {
        first.album.clone()
    } else {
        String::new()
    };
    let album_artist = first.album_artist.clone();

    let album_tracks =
        if distinct_albums.len() == 1 && !album_title.is_empty() && !album_artist.is_empty() {
            Some(crate::resolver::resolve_album_platform_tracks(&album_artist, &album_title).await)
        } else {
            None
        };

    let mut per_song_candidates: Vec<Vec<AudioCandidate>> = vec![Vec::new(); ordered_songs.len()];
    let mut unresolved_indices: Vec<usize> = Vec::new();

    for (idx, song) in ordered_songs.iter().enumerate() {
        let track_num = song.track_number.map(|n| n.max(0) as u32).unwrap_or((idx + 1) as u32);
        let disc_num = song.disc_number.map(|n| n.max(1) as u32);
        let mut candidates = if let Some(ref at) = album_tracks {
            crate::resolver::match_album_platform_candidates(
                &song.title,
                song.duration_ms,
                track_num,
                disc_num,
                at,
            )
        } else {
            Vec::new()
        };

        if candidates.is_empty() {
            unresolved_indices.push(idx);
        } else {
            arbiter::rank_candidates(&mut candidates);
            per_song_candidates[idx] = candidates;
        }
    }

    if !unresolved_indices.is_empty() {
        let mut set = tokio::task::JoinSet::new();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
        for idx in unresolved_indices {
            let song = ordered_songs[idx].clone();
            let permit = sem.clone();
            set.spawn(async move {
                let _guard = permit.acquire_owned().await;
                let artist = song
                    .artists
                    .first()
                    .filter(|a| !a.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| song.album_artist.clone());
                let cands = crate::resolver::resolve_candidates_for_track(
                    &artist,
                    &song.title,
                    song.duration_ms,
                )
                .await;
                (idx, cands)
            });
        }
        while let Some(res) = set.join_next().await {
            if let Ok((idx, cands)) = res {
                per_song_candidates[idx] = cands;
            }
        }
    }

    if !cache_key.is_empty() {
        let mut guard = upgrade_candidate_cache().lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() > 64 {
            guard.clear();
        }
        guard.insert(cache_key, (per_song_candidates.clone(), std::time::Instant::now()));
    }

    per_song_candidates
}

/// Probes multi-source candidates for existing library songs (e.g. from Upgrade Quality modal).
#[tauri::command]
pub async fn probe_upgrade_candidates(
    app: AppHandle,
    song_ids: Vec<i64>,
) -> Result<Vec<crate::download::album_preview::AlbumTrackInfo>, String> {
    let ordered_songs = load_ordered_songs(&app, &song_ids).await?;
    let per_song = resolve_upgrade_candidates_cached(&ordered_songs).await;
    Ok(ordered_songs
        .into_iter()
        .enumerate()
        .map(|(idx, song)| {
            let candidates = per_song.get(idx).cloned().unwrap_or_default();
            let primary_url = candidates
                .first()
                .map(|c| c.source.primary_url().to_string())
                .or(song.source_url.clone())
                .unwrap_or_default();
            let file_exists = Path::new(&song.path).exists();
            let note = file_exists.then(|| format!("local_tier:{}", quality_tier_slug(song.quality_tier)));
            crate::download::album_preview::AlbumTrackInfo {
                index: idx,
                title: song.title,
                artists: song.artists,
                track_number: song.track_number,
                disc_number: song.disc_number,
                url: primary_url.clone(),
                available: !primary_url.is_empty() || !candidates.is_empty(),
                note,
                duration_ms: (song.duration_ms > 0).then_some(song.duration_ms),
                candidates,
                selected_candidate_idx: 0,
            }
        })
        .collect())
}

async fn build_bulk_upgrade_album(
    app_handle: &AppHandle,
    task_id: &str,
    platform: Platform,
    url: &str,
    selected: Option<&[String]>,
    effective_format: Option<&str>,
    overrides: Option<&crate::download::types::DownloadOverrides>,
    fallback_output_dir: &Path,
) -> Result<(AlbumDownload, Option<PathBuf>), String> {
    use std::collections::HashSet;

    let mut song_ids: Vec<i64> = Vec::new();
    if let Some(items) = selected {
        for raw in items {
            if let Some(id_str) = raw.strip_prefix("resolve:song:") {
                if let Ok(id) = id_str.parse::<i64>() {
                    if !song_ids.contains(&id) {
                        song_ids.push(id);
                    }
                }
            }
        }
    }
    if song_ids.is_empty() {
        if let Some(id_str) = url.strip_prefix("resolve:song:") {
            if let Ok(id) = id_str.parse::<i64>() {
                song_ids.push(id);
            }
        }
    }
    let ordered_songs = load_ordered_songs(app_handle, &song_ids).await?;

    let first = ordered_songs[0].clone();
    let distinct_albums: HashSet<(String, String)> = ordered_songs
        .iter()
        .map(|s| (s.album_artist.to_lowercase(), s.album.to_lowercase()))
        .collect();
    let album_title = if distinct_albums.len() == 1 && !first.album.is_empty() {
        first.album.clone()
    } else {
        format!("Upgrading {} tracks", ordered_songs.len())
    };
    let album_artist = first.album_artist.clone();

    let _ = app_handle.emit(
        "download-task-progress",
        ProgressEvent {
            task_id: task_id.to_string(),
            current: 0,
            total: ordered_songs.len(),
            track_title: "Resolving best sources in bulk...".into(),
        },
    );

    let per_song_candidates = resolve_upgrade_candidates_cached(&ordered_songs).await;
    let override_by_idx: HashMap<usize, &crate::download::types::TrackOverride> = overrides
        .and_then(|o| o.tracks.as_ref())
        .map(|list| list.iter().filter_map(|ov| ov.index.map(|i| (i, ov))).collect())
        .unwrap_or_default();

    let mut tracks = Vec::with_capacity(ordered_songs.len());
    let mut replace_ids = HashMap::new();
    let mut skipped_same_quality = 0usize;
    for (idx, song) in ordered_songs.into_iter().enumerate() {
        let mut candidates = per_song_candidates[idx].clone();
        if let Some(ov) = override_by_idx.get(&idx) {
            if let Some(ref ov_cands) = ov.candidates {
                for cand in ov_cands {
                    let already = candidates.iter().any(|existing| existing.source == cand.source);
                    if !already {
                        candidates.push(cand.clone());
                    }
                }
                crate::resolver::arbiter::rank_candidates(&mut candidates);
            }
        }

        let local_exists = Path::new(&song.path).exists();
        let chosen = pick_candidate_entry_for_tier(&candidates, effective_format, 0);
        if let Some((chosen_idx, _, _)) = &chosen {
            let cand_tier = candidates[*chosen_idx].quality_tier;
            if !candidate_improves_local(song.quality_tier, local_exists, cand_tier) {
                tracing::info!(
                    title = %song.title,
                    local_tier = ?song.quality_tier,
                    candidate_tier = ?cand_tier,
                    "Skipping upgrade: local file already has equal or higher quality"
                );
                skipped_same_quality += 1;
                continue;
            }
        } else if local_exists {
            // No verified external candidate beats or replaces an existing local file cleanly.
            skipped_same_quality += 1;
            continue;
        }

        let mut track = TrackDownload {
            url: String::new(),
            file_name: String::new(),
            title: song.title,
            artists: song.artists,
            album_artist: song.album_artist,
            album: song.album,
            track_number: song.track_number,
            disc_number: song.disc_number,
            genres: song.genre.map(|g| vec![g]),
            release_year: song.release_year,
            source_item_id: None,
            duration_ms: Some(song.duration_ms),
            fallback_urls: Vec::new(),
            ..Default::default()
        };
        let _ = apply_candidate_to_track(&mut track, &candidates, effective_format, 0);

        if let Some(ref orig_url) = song.source_url {
            if orig_url.starts_with("http://") || orig_url.starts_with("https://") {
                if track.url.is_empty() {
                    track.url = orig_url.clone();
                } else if orig_url != &track.url && !track.fallback_urls.contains(orig_url) {
                    track.fallback_urls.push(orig_url.clone());
                }
            }
        }
        if track.url.is_empty() && track.torrent_target.is_none() {
            tracing::warn!(title = %track.title, "Skipping track without any resolved upgrade stream");
            continue;
        }
        if let Some(old_id) = song.id {
            replace_ids.insert(tracks.len(), old_id);
        }
        tracks.push(track);
    }

    if tracks.is_empty() && skipped_same_quality == 0 {
        return Err("Could not find matching streams on YouTube, Bandcamp, or SoundCloud for upgrade.".to_string());
    }

    if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
        registry.set_replace_song_ids(task_id, replace_ids).await;
    }

    let existing_dir = Path::new(&first.path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && p.exists())
        .map(Path::to_path_buf);

    let external_cover_url = ordered_external_cover(&first, platform, fallback_output_dir);

    Ok((
        AlbumDownload {
            title: album_title,
            artists: vec![album_artist.clone()],
            album_artist,
            tracks,
            genres: None,
            release_year: first.release_year,
            external_cover_url,
        },
        existing_dir,
    ))
}

fn ordered_external_cover(
    song: &crate::Song,
    _platform: Platform,
    _fallback_dir: &Path,
) -> Option<String> {
    song.external_cover_url.clone()
}

/// Downloads the album at `url`; with `selected` only the tracks whose page url is listed (the album
/// folder, cover and track numbers stay those of the whole album).
pub async fn process_album_url(
    platform: Platform,
    url: &str,
    selected: Option<&[String]>,
    output_dir: &str,
    app_handle: &AppHandle,
    task_id: &str,
) -> Result<DownloadResult, String> {
    let root_library_dir = PathBuf::from(output_dir);
    let task_registry_opt = app_handle.try_state::<crate::download::TaskRegistry>();
    let effective_format = match &task_registry_opt {
        Some(reg) => reg.format(task_id).await,
        None => None,
    };
    let task_overrides = match &task_registry_opt {
        Some(reg) => reg.overrides(task_id).await,
        None => None,
    };

    let is_upgrade = url.starts_with("resolve:");
    let mut skipped_note = None;
    let mut skipped_tracks: Vec<SkippedTrack> = Vec::new();
    let mut upgrade_target_dir: Option<PathBuf> = None;

    let mut canonical_urls = Vec::new();
    let mut album_info = if is_upgrade {
        let (info, dir) = build_bulk_upgrade_album(
            app_handle,
            task_id,
            platform,
            url,
            selected,
            effective_format.as_deref(),
            task_overrides.as_ref(),
            &root_library_dir,
        )
        .await?;
        upgrade_target_dir = dir;
        info
    } else if url.starts_with("canonical:") {
        let mbid = url.trim_start_matches("canonical:");
        let cache_dir = app_handle.path().app_cache_dir().ok();
        let client = crate::metadata::MusicBrainzClient::new(cache_dir.clone());
        let mut canonical = if mbid.starts_with("http://") || mbid.starts_with("https://") {
            let detected = detect_platform_from_url(mbid, platform);
            let synth_key = format!(
                "synth:{}:{}",
                detected,
                xxhash_rust::xxh3::xxh3_64(mbid.as_bytes())
            );
            if let Ok(cached) = client.fetch_canonical_release(&synth_key).await {
                cached
            } else {
                let raw = crate::download::album_preview::album_preview(detected, mbid).await?;
                crate::metadata::fallback::synthesize_canonical_from_preview(raw)
            }
        } else {
            client.fetch_canonical_release(mbid).await?
        };

        let needs_resolution = canonical
            .discs
            .iter()
            .any(|d| d.tracks.iter().any(|t| t.candidates.is_empty()));
        if needs_resolution {
            crate::resolver::resolve_release_candidates(&mut canonical, None, url).await;
            if let Ok(serialized) = serde_json::to_string(&canonical) {
                client
                    .write_cache(&format!("release_v7:{}", canonical.id), &serialized)
                    .await;
            }
        }

        let mut track_downloads = Vec::new();
        for disc in &canonical.discs {
            for t in &disc.tracks {
                let mut td = crate::download::TrackDownload {
                    url: String::new(),
                    file_name: String::new(),
                    title: t.title.clone(),
                    artists: t.artists.clone(),
                    album_artist: canonical.album_artist.clone(),
                    album: canonical.title.clone(),
                    track_number: Some(t.track_number as i32),
                    disc_number: Some(disc.disc_number as i32),
                    genres: None,
                    release_year: canonical.release_year.map(|y| y as i32),
                    source_item_id: None,
                    duration_ms: Some(t.duration_ms),
                    fallback_urls: Vec::new(),
                    ..Default::default()
                };
                canonical_urls.push(t.candidates.get(t.selected_candidate_idx)
                    .or_else(|| t.candidates.first())
                    .map(|candidate| candidate.source.primary_url().to_string())
                    .unwrap_or_default());
                let _ = apply_candidate_to_track(
                    &mut td,
                    &t.candidates,
                    effective_format.as_deref(),
                    t.selected_candidate_idx,
                );
                track_downloads.push(td);
            }
        }

        AlbumDownload {
            title: canonical.title,
            artists: vec![canonical.album_artist.clone()],
            album_artist: canonical.album_artist,
            tracks: track_downloads,
            genres: None,
            release_year: canonical.release_year.map(|y| y as i32),
            external_cover_url: canonical.cover_url,
        }
    } else {
        match platform {
            Platform::Bandcamp => bandcamp::parse_album(url).await?,
            Platform::Youtube => youtube::parse_album(url).await?,
            Platform::Soundcloud => {
                let (album, skipped) = soundcloud::parse_album_with_skipped(url).await?;
                if !skipped.is_empty() {
                    skipped_note = Some(soundcloud::describe_skipped(&skipped));
                }
                skipped_tracks = skipped;
                album
            }
            Platform::Local => {
                return Err("Local files cannot be processed via remote URL pipelines.".into());
            }
        }
    };

    let selected_positions = (!is_upgrade).then(|| selected.map(|urls|
        selected_track_positions(&album_info.tracks, &canonical_urls, urls)
    )).flatten();
    if !is_upgrade {
        if let Some(ref overrides) = task_overrides {
            apply_download_overrides_with_format(
                &mut album_info,
                overrides,
                effective_format.as_deref(),
            );
        }
    } else if let Some(ref overrides) = task_overrides {
        if let Some(ref title) = overrides.title {
            let trimmed = title.trim();
            if !trimmed.is_empty() {
                album_info.title = trimmed.to_string();
            }
        }
        if let Some(ref artist) = overrides.artist {
            let trimmed = artist.trim();
            if !trimmed.is_empty() {
                album_info.album_artist = trimmed.to_string();
                album_info.artists = vec![trimmed.to_string()];
            }
        }
        if let Some(year) = overrides.year {
            album_info.release_year = Some(year);
        }
    }

    let album_artist = album_info.album_artist.clone();
    for track in &mut album_info.tracks {
        track.album_artist = album_artist.clone();
    }
    if !is_upgrade {
        // Before the selection: the source holds the whole album, also when only some tracks are downloaded.
        record_album_source(app_handle, platform, url, &album_info, &skipped_tracks).await;
    }

    let mut tracks = album_info.tracks;
    for (index, track) in tracks.iter_mut().enumerate() {
        track.source_track_index = Some(index);
    }
    if !is_upgrade {
        if let Some(positions) = selected_positions {
            tracks = tracks.into_iter().enumerate().filter_map(|(index, track)|
                positions.contains(&index).then_some(track)
            ).collect();
            if tracks.is_empty() {
                return Err("None of the selected tracks was found in the album.".into());
            }
        } else if let Some(index) = library_index(app_handle, &album_info.album_artist, &album_info.title).await {
            // A whole-album download only fetches what the library does not have yet.
            tracks = without_owned(platform, &album_info.album_artist, tracks, &index);
            if tracks.is_empty() {
                return Err("All tracks of this album are already in your library.".into());
            }
        }
    }

    if !is_upgrade {
        if let Some(registry) = &task_registry_opt {
            if registry.uses_automatic_quality(task_id).await {
                super::discovery::discover_automatic_sources(
                    app_handle, task_id, &album_info.album_artist, &album_info.title, &mut tracks,
                    |_, track| track.torrent_target.is_some() || task_overrides.as_ref()
                        .and_then(|overrides| overrides.tracks.as_ref())
                        .is_some_and(|overrides| overrides.iter().any(|item| item.index == track.source_track_index &&
                            item.candidates.as_ref().is_some_and(|candidates| !candidates.is_empty()))),
                    effective_format.as_deref().unwrap_or("best"),
                ).await;
            }
        }
    }

    let _ = app_handle.emit(
        "download-album-meta",
        AlbumMetaEvent {
            task_id: task_id.to_string(),
            title: album_info.title.clone(),
            artist: album_info.album_artist.clone(),
            cover_url: album_info.external_cover_url.clone(),
            release_year: album_info.release_year,
            tracks: tracks
                .iter()
                .enumerate()
                .map(|(idx, t)| TrackMetaItem {
                    title: t.title.clone(),
                    track_number: t.track_number.or(Some((idx + 1) as i32)),
                    disc_number: t.disc_number,
                })
                .collect(),
        },
    );

    let target_library_dir = upgrade_target_dir.unwrap_or_else(|| {
        root_library_dir
            .join(sanitize_fs(&album_info.album_artist))
            .join(sanitize_fs(&album_info.title))
    });

    if is_upgrade && tracks.is_empty() {
        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.to_string(),
                current: 1,
                total: 1,
                track_title: "Already at the best available quality".into(),
            },
        );
        return Ok((target_library_dir, Vec::new(), None));
    }

    // Only a folder this download creates may be deleted again on cancel or failure: an existing album folder
    // holds songs of the library.
    let folder_created =
        !is_upgrade && target_library_dir != root_library_dir && !target_library_dir.exists();

    if let Some(registry) = app_handle.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_metadata(
                task_id,
                &album_info.title,
                &album_info.album_artist,
                album_info.external_cover_url.as_deref(),
                album_info.release_year,
                tracks
                    .iter()
                    .map(|t| (t.title.clone(), t.track_number, t.disc_number))
                    .collect(),
            )
            .await;
        if folder_created {
            registry
                .set_target_dir(
                    task_id,
                    target_library_dir.clone(),
                    root_library_dir.clone(),
                )
                .await;
        }
        if let Some(note) = &skipped_note {
            registry
                .update_task_progress(task_id, 0, tracks.len(), note)
                .await;
        }
    }

    if let Some(note) = skipped_note {
        let _ = app_handle.emit(
            "download-task-progress",
            ProgressEvent {
                task_id: task_id.to_string(),
                current: 0,
                total: tracks.len(),
                track_title: note,
            },
        );
    }

    if target_library_dir != root_library_dir {
        tokio::fs::create_dir_all(&target_library_dir)
            .await
            .map_err(|e| format!("Failed creating album path: {e}"))?;
    }

    let mut cover_path = None;
    if let Some(cover_url) = &album_info.external_cover_url {
        if download_album_cover(cover_url, &target_library_dir)
            .await
            .is_ok()
        {
            cover_path = Some(target_library_dir.join("cover.jpg"));
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(PathBuf, TrackDownload, usize)>(32);
    let app_handle_clone = app_handle.clone();
    let task_id_str = task_id.to_string();
    let target_dir_clone = target_library_dir.clone();
    let cover_path_clone = cover_path.clone();
    let covers_dir = app_handle
        .path()
        .app_data_dir()
        .map(|d| d.join("covers"))
        .unwrap_or_else(|_| PathBuf::from("covers"));
    let _ = fs::create_dir_all(&covers_dir);

    let consumer = async move {
        let mut songs = Vec::new();
        while let Some((temp_file, track, index)) = rx.recv().await {
            let track_platform = detect_platform_from_url(&track.url, platform);
            match crate::download::orchestrator::save_downloaded_track(
                &temp_file,
                &track,
                &target_dir_clone,
                cover_path_clone.as_deref(),
                &covers_dir,
                track_platform,
                &app_handle_clone,
                &task_id_str,
                index,
            )
            .await
            {
                Ok(song) => songs.push(song),
                Err(err) => tracing::error!(
                    title = %track.title,
                    error = %err,
                    "Failed to save downloaded track"
                ),
            }
        }
        songs
    };

    for (idx, t) in tracks.iter_mut().enumerate() {
        t.source_track_index = Some(idx);
    }

    let total_tracks = tracks.len();
    let completed_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut torrent_groups: std::collections::BTreeMap<
        String,
        (String, Vec<crate::torrent::download::TorrentFileRequest>),
    > = std::collections::BTreeMap::new();
    let mut http_tracks: Vec<TrackDownload> = Vec::new();

    for t in &tracks {
        if let Some(ref target) = t.torrent_target {
            let entry = torrent_groups
                .entry(target.info_hash.clone())
                .or_insert_with(|| (target.magnet_uri.clone(), Vec::new()));
            entry.1.push(crate::torrent::download::TorrentFileRequest {
                track_index: t.source_track_index.unwrap_or(0),
                file_index: target.file_index,
                file_size: target.file_size,
                track: t.clone(),
            });
        } else {
            http_tracks.push(t.clone());
        }
    }

    let torrent_staging = crate::torrent::download::torrent_cache_root(&root_library_dir);
    let task_staging = crate::torrent::download::torrent_staging_dir(&torrent_staging, task_id);
    let _staging = crate::torrent::download::TorrentStagingCleanup(task_staging);

    let from_torrent = super::youtube_sign_in::is_torrent_download(effective_format.as_deref(), &tracks);

    let producer = async {
        let mut transfer_err: Option<String> = None;
        for (_hash, (locator, requests)) in torrent_groups {
            match crate::torrent::download::download_torrent_files(
                app_handle,
                task_id,
                &root_library_dir,
                &locator,
                &requests,
                total_tracks,
                std::sync::Arc::clone(&completed_counter),
                Some(tx.clone()),
            )
            .await
            {
                Ok(res) => {
                    if transfer_err.is_none() {
                        transfer_err = torrent_transfer_error(&res);
                    }
                }
                Err(err) => {
                    if err == "Task cancelled" {
                        transfer_err = Some(err);
                        http_tracks.clear();
                        break;
                    }
                    if transfer_err.is_none() {
                        transfer_err = Some(err);
                    }
                }
            }
        }

        if from_torrent {
            let skipped = super::youtube_sign_in::leave_out_youtube_unless_signed_in(
                &mut http_tracks,
                super::youtube_sign_in::has_session(app_handle),
            ).await;
            if skipped > 0 {
                transfer_err.get_or_insert_with(|| super::youtube_sign_in::skipped_message(skipped));
            }
        }

        let download_res = if !http_tracks.is_empty() {
            ytdlp::download_batch_streaming(
                app_handle,
                &http_tracks,
                output_dir,
                task_id,
                Some(tx),
            )
            .await
        } else {
            drop(tx);
            match transfer_err.as_ref() {
                Some(err) => Err(err.clone()),
                None => Ok(Vec::new()),
            }
        };

        (download_res, transfer_err)
    };
    let ((download_res, transfer_err), saved_songs) = tokio::join!(producer, consumer);

    match download_res {
        Ok(files) => {
            if let Some((first, _)) = files.first() {
                if let Some(parent) = first.parent() {
                    let _ = fs::remove_dir_all(parent);
                }
            }
            if let Some(err) = transfer_err {
                return Err(err);
            }
            Ok((target_library_dir, Vec::new(), cover_path))
        }
        Err(err) => {
            if saved_songs.is_empty() {
                if folder_created {
                    discard_album_folder(&target_library_dir, &root_library_dir);
                }
                return Err(transfer_err.unwrap_or(err));
            }
            match transfer_err {
                Some(torrent_err) => Err(torrent_err),
                None => Ok((target_library_dir, Vec::new(), cover_path)),
            }
        }
    }
}

/// Removes an album folder created by a download that saved no songs.
fn discard_album_folder(target_dir: &Path, root_dir: &Path) {
    if target_dir.exists() && fs::remove_dir_all(target_dir).is_ok() {
        remove_empty_parents_up_to(target_dir, root_dir);
    }
}

fn torrent_transfer_error(result: &crate::torrent::download::TorrentTransferResult) -> Option<String> {
    let failed_count = result.failed_tracks.len();
    if failed_count == 0 {
        return None;
    }
    let reason = result.failure.as_deref().unwrap_or("Some torrent files could not be downloaded.");
    Some(format!("{reason} {failed_count} track(s) failed. Completed tracks were kept."))
}

pub async fn download_album_cover(url: &str, output_dir: &Path) -> Result<(), String> {
    let bytes = reqwest::get(url)
        .await
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    tokio::fs::write(output_dir.join("cover.jpg"), &bytes)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::db::tests::setup_test_db;
    use crate::library_index::normalize::normalize_url_key;

    #[test]
    fn discarding_a_failed_album_removes_its_cover_and_empty_parents_only() {
        let root = tempfile::tempdir().unwrap();
        let album = root.path().join("Artist").join("Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("cover.jpg"), b"cover").unwrap();
        discard_album_folder(&album, root.path());
        assert!(!root.path().join("Artist").exists());
        assert!(root.path().exists());
    }

    fn album_of(urls: &[&str]) -> AlbumDownload {
        AlbumDownload {
            title: "The Album".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            tracks: urls.iter().enumerate().map(|(i, u)| track(u, i as i32 + 1)).collect(),
            genres: None,
            release_year: Some(2020),
            external_cover_url: Some("https://img/c.jpg".to_string()),
        }
    }

    async fn db_with_schema() -> Database {
        let db = setup_test_db().await;
        db.ensure_album_sources_schema().await.unwrap();
        db
    }

    #[tokio::test]
    async fn write_album_source_stores_the_whole_album() {
        let db = db_with_schema().await;
        let url = "https://a.bandcamp.com/album/the-album";
        let album = album_of(&["https://a.bandcamp.com/track/1", "https://a.bandcamp.com/track/2"]);
        assert!(write_album_source(&db, Platform::Bandcamp, url, &album, &[], 42).await.unwrap());

        let row = db
            .get_album_source_by_key(&normalize_url_key(url).unwrap())
            .await
            .unwrap()
            .expect("row");
        assert_eq!(row.platform, "bandcamp");
        assert_eq!(row.album, "The Album");
        assert_eq!(row.album_artist, "Artist");
        assert_eq!(row.fetched_at, 42);
        assert_eq!(
            row.track_urls,
            r#"["https://a.bandcamp.com/track/1","https://a.bandcamp.com/track/2"]"#
        );
    }

    #[tokio::test]
    async fn write_album_source_updates_the_row_on_a_second_download() {
        let db = db_with_schema().await;
        let url = "https://a.bandcamp.com/album/the-album";
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1"]), &[], 1).await.unwrap();
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1", "https://u/2"]), &[], 2)
            .await
            .unwrap();
        let rows = db.list_album_sources_for_artist("Artist").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fetched_at, 2);
        assert_eq!(rows[0].track_urls, r#"["https://u/1","https://u/2"]"#);
    }

    #[tokio::test]
    async fn write_album_source_skips_albums_without_a_usable_key_or_tracks() {
        let db = db_with_schema().await;
        assert!(!write_album_source(&db, Platform::Bandcamp, "", &album_of(&["https://u/1"]), &[], 1).await.unwrap());
        assert!(!write_album_source(&db, Platform::Bandcamp, "https://a.bandcamp.com/album/x", &album_of(&[]), &[], 1)
            .await
            .unwrap());
        assert!(db.list_album_sources_for_artist("Artist").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn write_album_source_reports_a_missing_table_instead_of_panicking() {
        let db = setup_test_db().await; // no album_sources table
        let result = write_album_source(
            &db,
            Platform::Bandcamp,
            "https://a.bandcamp.com/album/x",
            &album_of(&["https://u/1"]),
            &[],
            1,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn downloading_the_album_from_another_platform_replaces_the_old_link() {
        let db = db_with_schema().await;
        let soundcloud = "https://soundcloud.com/artist/sets/the-album";
        write_album_source(&db, Platform::Bandcamp, "https://a.bandcamp.com/album/the-album", &album_of(&["https://a.bandcamp.com/track/1"]), &[], 1)
            .await
            .unwrap();
        write_album_source(&db, Platform::Soundcloud, soundcloud, &album_of(&["https://sc/1"]), &[], 2).await.unwrap();

        let rows = db.list_album_sources_for_artist("Artist").await.unwrap();
        assert_eq!(rows.len(), 1, "the old platform link is replaced");
        assert_eq!(rows[0].platform, "soundcloud");
        assert_eq!(rows[0].album_url, soundcloud);
        assert_eq!(rows[0].track_urls, r#"["https://sc/1"]"#);
        assert_eq!(rows[0].fetched_at, 2);
    }

    #[tokio::test]
    async fn downloading_a_different_album_of_the_same_artist_keeps_the_first_row() {
        let db = db_with_schema().await;
        write_album_source(&db, Platform::Bandcamp, "https://a.bandcamp.com/album/the-album", &album_of(&["https://u/1"]), &[], 1)
            .await
            .unwrap();
        let mut other = album_of(&["https://u/2"]);
        other.title = "Another Album".to_string();
        write_album_source(&db, Platform::Soundcloud, "https://soundcloud.com/artist/sets/another", &other, &[], 2)
            .await
            .unwrap();

        let rows = db.list_album_sources_for_artist("Artist").await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.album == "The Album" && r.platform == "bandcamp"));
        assert!(rows.iter().any(|r| r.album == "Another Album" && r.platform == "soundcloud"));
    }

    #[tokio::test]
    async fn redownloading_the_same_url_leaves_one_row() {
        let db = db_with_schema().await;
        let url = "https://a.bandcamp.com/album/the-album";
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1"]), &[], 1).await.unwrap();
        write_album_source(&db, Platform::Bandcamp, url, &album_of(&["https://u/1"]), &[], 2).await.unwrap();

        let rows = db.list_album_sources_for_artist("Artist").await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].fetched_at, 2);
    }

    fn track(url: &str, number: i32) -> TrackDownload {
        TrackDownload {
            url: url.to_string(),
            title: format!("t{number}"),
            track_number: Some(number),
            ..TrackDownload::default()
        }
    }

    #[test]
    fn select_tracks_keeps_album_order_and_numbers() {
        let tracks = vec![track("a", 1), track("b", 2), track("c", 3)];
        let picked = select_tracks(tracks, &["c".to_string(), "a".to_string()]);
        let urls: Vec<&str> = picked.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "c"], "album order, not selection order");
        assert_eq!(picked[1].track_number, Some(3), "numbers of the whole album are kept");
    }

    #[test]
    fn select_tracks_matches_urls_that_differ_only_in_form() {
        let picked = select_tracks(
            vec![track("https://a.bandcamp.com/track/x", 1)],
            &["https://a.bandcamp.com/track/x/?from=search".to_string()],
        );
        assert_eq!(picked.len(), 1);
    }

    #[test]
    fn without_owned_keeps_only_the_missing_tracks() {
        let tracks = vec![
            TrackDownload { title: "One".into(), ..track("https://u/1", 1) },
            TrackDownload { title: "Two".into(), ..track("https://u/2", 2) },
        ];
        let index = crate::library_index::status::LibraryIndex::default()
            .with_titles([("Artist".to_string(), "One".to_string())]);
        let left = without_owned(Platform::Bandcamp, "Artist", tracks, &index);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].url, "https://u/2");
    }

    #[test]
    fn select_tracks_ignores_unknown_urls_and_can_be_empty() {
        let picked = select_tracks(vec![track("a", 1)], &["zzz".to_string()]);
        assert!(picked.is_empty());
        assert!(select_tracks(vec![track("a", 1)], &[]).is_empty());
    }

    #[test]
    fn pick_candidate_urls_respects_tier_and_source_specs() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
        let cand = |plat: &str, url: &str, tier: AudioQualityTier| AudioCandidate {
            source: AudioSource::HttpStream {
                platform: plat.into(),
                url: url.into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: tier,
            codec: "Test".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: plat.into(),
            matched_title: Some("Song".into()),
            matched_artist: Some("Artist".into()),
        };
        let list = vec![
            cand("bandcamp", "https://bc.bandcamp.com/track/1", AudioQualityTier::Lossless),
            cand("youtube", "https://youtube.com/watch?v=1", AudioQualityTier::HighLossy),
            cand("soundcloud", "https://soundcloud.com/a/1", AudioQualityTier::StandardLossy),
        ];

        assert_eq!(pick_candidate_urls_for_tier(&list, Some("best"), 0).0, "https://bc.bandcamp.com/track/1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("high"), 0).0, "https://youtube.com/watch?v=1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("standard"), 0).0, "https://soundcloud.com/a/1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("source:youtube"), 0).0, "https://youtube.com/watch?v=1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("best"), 1).0, "https://youtube.com/watch?v=1");
    }

    #[test]
    fn apply_download_overrides_updates_album_and_track_urls() {
        let mut album = album_of(&["https://orig/1", "https://orig/2"]);
        let overrides = crate::download::types::DownloadOverrides {
            title: Some("Edited Album".into()),
            artist: Some("Edited Artist".into()),
            year: Some(1999),
            tracks: Some(vec![crate::download::types::TrackOverride {
                index: Some(1),
                title: Some("Edited Track 2".into()),
                url: Some("https://picked/2".into()),
                fallback_urls: Some(vec!["https://fallback/2".into()]),
                ..Default::default()
            }]),
        };
        apply_download_overrides(&mut album, &overrides);

        assert_eq!(album.title, "Edited Album");
        assert_eq!(album.album_artist, "Edited Artist");
        assert_eq!(album.release_year, Some(1999));
        assert_eq!(album.tracks[0].album, "Edited Album");
        assert_eq!(album.tracks[1].title, "Edited Track 2");
        assert_eq!(album.tracks[1].url, "https://picked/2");
        assert_eq!(album.tracks[1].fallback_urls, vec!["https://fallback/2".to_string()]);
    }

    #[test]
    fn selected_original_tracks_keep_their_indices_after_switching_platforms() {
        let mut album = album_of(&["https://orig/1", "https://orig/2"]);
        let urls = vec!["https://orig/2".to_string()];
        let positions = selected_track_positions(&album.tracks, &[], &urls);
        apply_download_overrides(&mut album, &crate::download::types::DownloadOverrides {
            tracks: Some(vec![crate::download::types::TrackOverride {
                index: Some(1), url: Some("https://youtube.com/watch?v=chosen".into()), ..Default::default()
            }]), ..Default::default()
        });
        let result: Vec<_> = album.tracks.iter().enumerate().filter_map(|(index, track)|
            positions.contains(&index).then_some(track.url.as_str())).collect();
        assert_eq!(result, ["https://youtube.com/watch?v=chosen"]);
        assert_eq!(selected_track_positions(&album.tracks, &["https://orig/1".into(), "https://orig/2".into()], &urls), positions);
    }

    #[test]
    fn scoped_override_follows_its_url_past_unavailable_preview_rows() {
        let mut album = album_of(&["https://orig/1", "https://orig/2"]);
        apply_download_overrides(&mut album, &crate::download::types::DownloadOverrides {
            tracks: Some(vec![crate::download::types::TrackOverride {
                index: Some(2),
                original_url: Some("https://orig/2".into()),
                url: Some("https://youtube.com/watch?v=chosen".into()),
                ..Default::default()
            }]),
            ..Default::default()
        });
        assert_eq!(album.tracks[0].url, "https://orig/1");
        assert_eq!(album.tracks[1].url, "https://youtube.com/watch?v=chosen");
    }

    #[test]
    fn detect_platform_from_url_identifies_source_host() {
        assert_eq!(detect_platform_from_url("https://artist.bandcamp.com/track/x", Platform::Youtube), Platform::Bandcamp);
        assert_eq!(detect_platform_from_url("https://soundcloud.com/a/b", Platform::Youtube), Platform::Soundcloud);
        assert_eq!(detect_platform_from_url("https://youtu.be/abc", Platform::Bandcamp), Platform::Youtube);
    }

    #[test]
    fn candidate_improves_local_checks_file_existence_and_strict_tier_increase() {
        use crate::metadata::types::AudioQualityTier;
        use crate::types::QualityTier;

        // Missing local file should always be re-downloadable regardless of recorded tier.
        assert!(candidate_improves_local(QualityTier::Lossless, false, AudioQualityTier::StandardLossy));

        // Same or lower tier when local file exists should skip upgrade.
        assert!(!candidate_improves_local(QualityTier::High, true, AudioQualityTier::HighLossy));
        assert!(!candidate_improves_local(QualityTier::High, true, AudioQualityTier::StandardLossy));
        assert!(!candidate_improves_local(QualityTier::Lossless, true, AudioQualityTier::HighLossy));
        assert!(!candidate_improves_local(QualityTier::Lossless, true, AudioQualityTier::Lossless));

        // Strictly higher tier improves the local file.
        assert!(candidate_improves_local(QualityTier::Low, true, AudioQualityTier::StandardLossy));
        assert!(candidate_improves_local(QualityTier::Standard, true, AudioQualityTier::HighLossy));
        assert!(candidate_improves_local(QualityTier::High, true, AudioQualityTier::Lossless));
    }

    #[test]
    fn pick_candidate_entry_for_tier_returns_chosen_index_alongside_urls() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let cand = |plat: &str, url: &str, tier: AudioQualityTier| AudioCandidate {
            source: AudioSource::HttpStream {
                platform: plat.into(),
                url: url.into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: tier,
            codec: "Test".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: plat.into(),
            matched_title: Some("Song".into()),
            matched_artist: Some("Artist".into()),
        };
        let list = vec![
            cand("bandcamp", "https://bc.bandcamp.com/track/1", AudioQualityTier::Lossless),
            cand("youtube", "https://youtube.com/watch?v=1", AudioQualityTier::HighLossy),
        ];

        let (idx_best, primary_best, fallbacks_best) =
            pick_candidate_entry_for_tier(&list, Some("best"), 0).unwrap();
        assert_eq!(idx_best, 0);
        assert_eq!(primary_best, "https://bc.bandcamp.com/track/1");
        assert_eq!(fallbacks_best, vec!["https://youtube.com/watch?v=1"]);

        let (idx_high, primary_high, _) =
            pick_candidate_entry_for_tier(&list, Some("high"), 0).unwrap();
        assert_eq!(idx_high, 1);
        assert_eq!(primary_high, "https://youtube.com/watch?v=1");
    }

    #[test]
    fn pick_candidate_entry_for_tier_selects_specific_torrent_info_hash_when_given() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let tor = |hash: &str| AudioCandidate {
            source: AudioSource::Torrent {
                info_hash: hash.into(),
                magnet_uri: format!("magnet:?xt=urn:btih:{hash}"),
                file_index: 0,
                file_path: "01.flac".into(),
                file_size: 1000,
                seeders: 10,
            },
            quality_tier: AudioQualityTier::Lossless,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: "TORRENT".into(),
            matched_title: None,
            matched_artist: None,
        };
        let list = vec![tor("archive-hash"), tor("rutracker-hash")];
        let (idx, _, _) =
            pick_candidate_entry_for_tier(&list, Some("source:torrent:lossless:rutracker-hash"), 0)
                .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn apply_candidate_to_track_sets_torrent_target_while_keeping_http_fallbacks() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let mut td = track("https://youtube.com/watch?v=orig", 1);
        let candidates = vec![
            AudioCandidate {
                source: AudioSource::Torrent {
                    info_hash: "aaaa1111".into(),
                    magnet_uri: "magnet:?xt=urn:btih:aaaa1111".into(),
                    file_index: 3,
                    file_path: "01 Track.flac".into(),
                    file_size: 25_000_000,
                    seeders: 10,
                },
                quality_tier: AudioQualityTier::Lossless,
                codec: "FLAC".into(),
                bitrate_kbps: None,
                sample_rate_hz: None,
                duration_ms: 180_000,
                match_confidence: 1.0,
                is_strict_match: true,
                display_label: "Torrent · FLAC".into(),
                matched_title: Some("t1".into()),
                matched_artist: None,
            },
            AudioCandidate {
                source: AudioSource::HttpStream {
                    platform: "youtube".into(),
                    url: "https://youtube.com/watch?v=orig".into(),
                    format_id: None,
                    direct_stream_url: None,
                    expires_at: None,
                },
                quality_tier: AudioQualityTier::HighLossy,
                codec: "Opus".into(),
                bitrate_kbps: Some(160),
                sample_rate_hz: Some(48_000),
                duration_ms: 180_000,
                match_confidence: 0.98,
                is_strict_match: true,
                display_label: "YouTube · Opus".into(),
                matched_title: Some("t1".into()),
                matched_artist: None,
            },
        ];

        apply_candidate_to_track(&mut td, &candidates, Some("source:torrent:lossless"), 0);
        assert_eq!(td.url, "https://youtube.com/watch?v=orig");
        assert_eq!(td.fallback_urls, vec!["https://youtube.com/watch?v=orig".to_string()]);
        let tt = td.torrent_target.as_ref().expect("torrent target set");
        assert_eq!(tt.file_index, 3);
        assert_eq!(tt.info_hash, "aaaa1111");

        apply_candidate_to_track(&mut td, &candidates, Some("source:youtube:high"), 0);
        assert!(td.torrent_target.is_none());
        assert_eq!(td.url, "https://youtube.com/watch?v=orig");
    }
}

#[cfg(test)]
mod torrent_failure_tests {
    use super::torrent_transfer_error;
    use crate::download::types::TrackDownload;
    use crate::torrent::download::TorrentTransferResult;

    #[test]
    fn failed_torrent_reports_error_even_when_http_fallback_exists() {
        let track = TrackDownload {
            url: "https://example.com/stream".into(),
            fallback_urls: vec!["https://example.com/other".into()],
            ..Default::default()
        };
        let result = TorrentTransferResult {
            completed: Vec::new(),
            failed_tracks: vec![(0, track)],
            failure: Some("Torrent download stalled.".into()),
        };
        let message = torrent_transfer_error(&result).unwrap();
        assert!(message.contains("Torrent download stalled."));
        assert!(message.contains("1 track(s) failed"));
    }

    #[test]
    fn completed_torrent_has_no_transfer_error() {
        let result = TorrentTransferResult { completed: Vec::new(), failed_tracks: Vec::new(), failure: None };
        assert!(torrent_transfer_error(&result).is_none());
    }
}
