use super::{inspect, matcher, search};
use crate::db::db::Database;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State, WebviewWindow};
use tokio::sync::oneshot;
use uuid::Uuid;

type DbLock = tokio::sync::Mutex<Database>;
const DISABLED_INDEXERS_KEY: &str = "disabled_torrent_indexers";
const RUTRACKER_ENABLED_KEY: &str = "rutracker_enabled";
const RUTRACKER_USERNAME_KEY: &str = "rutracker_username";

#[derive(Serialize)]
pub(crate) struct TorrentIndexer {
    id: search::Provider,
    name: &'static str,
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct TorrentIndexerTest {
    hits: usize,
    error: Option<String>,
}

async fn disabled_indexers(db: &Database) -> Result<Vec<search::Provider>, String> {
    let value = db.get_app_setting(DISABLED_INDEXERS_KEY).await
        .map_err(|_| "Could not load torrent indexer settings.")?;
    match value {
        Some(value) => serde_json::from_str(&value).map_err(|_| "Could not read torrent indexer settings.".into()),
        None => Ok(Vec::new()),
    }
}

async fn indexers(db: &Database, disabled: &[search::Provider]) -> Result<Vec<TorrentIndexer>, String> {
    let rutracker = db.get_app_setting(RUTRACKER_ENABLED_KEY).await
        .map_err(|_| "Could not load RuTracker settings.")?.as_deref() == Some("true");
    Ok(search::Provider::ALL.into_iter().chain([search::Provider::RuTracker]).map(|id| TorrentIndexer {
        id,
        name: id.name(),
        enabled: if id == search::Provider::RuTracker { rutracker } else { !disabled.contains(&id) },
    }).collect())
}

async fn enabled_indexers(db: &DbLock) -> Result<Vec<search::Provider>, String> {
    let db = db.lock().await;
    let disabled = disabled_indexers(&db).await?;
    Ok(indexers(&db, &disabled).await?.into_iter().filter(|indexer| indexer.enabled).map(|indexer| indexer.id).collect())
}

fn alternate_track_query<'a>(
    album: &str,
    expected: &'a [matcher::ExpectedTrackInput],
    releases: &[matcher::TorrentTierResolution],
) -> Option<&'a matcher::ExpectedTrackInput> {
    use crate::metadata::types::AudioQualityTier;
    use crate::resolver::matcher::clean_title;
    if expected.is_empty() || expected.len() > 3 {
        return None;
    }
    let album = clean_title(album);
    expected.iter().filter(|track| {
        let title = clean_title(&track.title);
        title != album && title.chars().filter(|ch| ch.is_alphabetic()).take(3).count() >= 3
            && !releases.iter().flat_map(|release| &release.tracks).any(|matched| {
                matched.expected_index == track.index && matched.candidate.is_strict_match
                    && matched.candidate.quality_tier == AudioQualityTier::Lossless
            })
    }).max_by_key(|track| track.title.len())
}

fn keep_exact_song_releases(
    releases: &mut Vec<matcher::TorrentTierResolution>,
    expected: &matcher::ExpectedTrackInput,
    artist: &str,
) {
    use crate::resolver::matcher::{clean_title, strip_leading_artist_from_title};
    let title = clean_title(&expected.title);
    releases.retain(|release| release.tracks.iter().any(|matched| {
        matched.expected_index == expected.index && matched.candidate.is_strict_match
            && matched.candidate.matched_title.as_ref().is_some_and(|name| {
                clean_title(&strip_leading_artist_from_title(artist, name)) == title
            })
    }));
}

pub(crate) async fn resolve_enabled_torrents(
    app: &tauri::AppHandle,
    artist: &str,
    title: &str,
    expected: &[matcher::ExpectedTrackInput],
    publish: impl Fn(matcher::TorrentResolveEvent),
) -> Result<(Vec<matcher::TorrentTierResolution>, Vec<String>), String> {
    let db = app.try_state::<DbLock>().ok_or("Torrent settings are unavailable.")?;
    let enabled = enabled_indexers(&db).await?;
    use matcher::TorrentResolveEvent::{Resolved, Verified};
    if alternate_track_query(title, expected, &[]).is_none() || enabled.is_empty() {
        return matcher::resolve_with_indexers(Some(app), artist, title, expected, &enabled, publish).await;
    }

    let combined = Mutex::new(Vec::<matcher::TorrentTierResolution>::new());
    let update = |incoming: Vec<matcher::TorrentTierResolution>| {
        let Ok(mut results) = combined.lock() else { return };
        for release in incoming {
            if let Some(previous) = results.iter_mut().find(|row| row.info_hash == release.info_hash && row.tier == release.tier) {
                if release.matched_count > previous.matched_count {
                    *previous = release;
                }
            } else {
                results.push(release);
            }
        }
        if !results.is_empty() {
            publish(Verified { resolutions: matcher::sorted_resolutions(results.clone()) });
        }
    };
    let primary = matcher::resolve_with_indexers(Some(app), artist, title, expected, &enabled, |event| match event {
        Verified { resolutions } | Resolved { resolutions, .. } => update(resolutions),
        other => publish(other),
    });
    let alternate = async {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let track = combined.lock().ok().and_then(|results| alternate_track_query(title, expected, &results));
        let Some(track) = track else { return };
        let artist = track.artist.as_deref().filter(|name| !name.trim().is_empty()).unwrap_or(artist);
        let _ = matcher::resolve_with_indexers(Some(app), artist, &track.title, std::slice::from_ref(track), &enabled, |event| {
            if let Verified { mut resolutions } | Resolved { mut resolutions, .. } = event {
                keep_exact_song_releases(&mut resolutions, track, artist);
                for resolution in &mut resolutions {
                    resolution.total_expected = expected.len();
                    resolution.missing_titles = expected.iter().filter(|other| other.index != track.index)
                        .map(|other| other.title.clone()).collect();
                }
                if !resolutions.is_empty() {
                    update(resolutions);
                }
            }
        }).await;
    };
    let (primary, _) = tokio::join!(primary, alternate);
    let resolutions = matcher::sorted_resolutions(combined.into_inner().unwrap_or_default());
    let unavailable_providers = match primary {
        Ok((_, unavailable)) => unavailable,
        Err(error) if resolutions.is_empty() => return Err(error),
        Err(_) => Vec::new(),
    };
    publish(Resolved { resolutions: resolutions.clone(), unavailable_providers: unavailable_providers.clone() });
    Ok((resolutions, unavailable_providers))
}

async fn save_indexer(
    db: &Database,
    id: search::Provider,
    enabled: bool,
) -> Result<Vec<TorrentIndexer>, String> {
    let mut disabled = disabled_indexers(db).await?;
    if id == search::Provider::RuTracker {
        db.set_app_setting(RUTRACKER_ENABLED_KEY, if enabled { "true" } else { "false" }).await
            .map_err(|_| "Could not save RuTracker settings.")?;
        return indexers(db, &disabled).await;
    }
    if enabled {
        disabled.retain(|provider| *provider != id);
    } else if !disabled.contains(&id) {
        disabled.push(id);
    }
    let value = serde_json::to_string(&disabled).map_err(|_| "Could not encode torrent indexer settings.")?;
    db.set_app_setting(DISABLED_INDEXERS_KEY, &value).await
        .map_err(|_| "Could not save torrent indexer settings.")?;
    indexers(db, &disabled).await
}

fn require_settings_window(label: &str) -> Result<(), String> {
    if label == "settings" {
        Ok(())
    } else {
        Err("Torrent indexers are configured from the Settings window.".into())
    }
}

#[tauri::command]
pub(crate) async fn list_torrent_indexers(
    window: WebviewWindow,
    db: State<'_, DbLock>,
) -> Result<Vec<TorrentIndexer>, String> {
    require_settings_window(window.label())?;
    let db = db.lock().await;
    indexers(&db, &disabled_indexers(&db).await?).await
}

#[tauri::command]
pub(crate) async fn set_torrent_indexer_enabled(
    window: WebviewWindow,
    db: State<'_, DbLock>,
    id: search::Provider,
    enabled: bool,
) -> Result<Vec<TorrentIndexer>, String> {
    require_settings_window(window.label())?;
    let updated = {
        let db = db.lock().await;
        save_indexer(&db, id, enabled).await?
    };
    let _ = window.emit_to("main", "torrent-indexers-changed", ());
    Ok(updated)
}

#[tauri::command]
pub(crate) async fn test_torrent_indexer(
    window: WebviewWindow,
    id: search::Provider,
) -> Result<TorrentIndexerTest, String> {
    require_settings_window(window.label())?;
    let report = search::test_provider(id, Some(window.app_handle())).await?;
    if id == search::Provider::RuTracker && report.error.is_none() {
        let _ = window.emit_to("main", "torrent-indexers-changed", ());
    }
    Ok(TorrentIndexerTest { hits: report.hits.len(), error: report.error })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuTrackerStatus {
    signed_in: bool,
    username: Option<String>,
    has_saved_credentials: bool,
}

#[cfg(not(test))]
pub(crate) async fn record_rutracker_login(app: &tauri::AppHandle, username: &str) {
    let Some(db) = app.try_state::<DbLock>() else { return };
    let db = db.lock().await;
    let _ = db.set_app_setting(RUTRACKER_ENABLED_KEY, "true").await;
    let _ = db.set_app_setting(RUTRACKER_USERNAME_KEY, username).await;
}

#[tauri::command]
pub(crate) async fn get_rutracker_status(
    window: WebviewWindow,
    db: State<'_, DbLock>,
) -> Result<RuTrackerStatus, String> {
    require_settings_window(window.label())?;
    let live_username = super::rutracker::live_session_username(window.app_handle()).await;
    let saved_username = super::rutracker::saved_username();
    let has_saved_credentials = super::rutracker::has_saved_credentials();
    let stored_username = {
        let db = db.lock().await;
        if let Some(ref name) = live_username {
            let _ = db.set_app_setting(RUTRACKER_USERNAME_KEY, name).await;
        }
        db.get_app_setting(RUTRACKER_USERNAME_KEY)
            .await
            .map_err(|_| "Could not load RuTracker status.")?
            .filter(|name| !name.trim().is_empty())
    };
    let username = live_username.or(saved_username).or(stored_username);
    let signed_in = username.is_some();
    Ok(RuTrackerStatus {
        signed_in,
        username,
        has_saved_credentials,
    })
}

#[tauri::command]
pub(crate) async fn sign_in_rutracker(
    window: WebviewWindow,
    db: State<'_, DbLock>,
    username: String,
    password: String,
) -> Result<super::rutracker::RuTrackerSignInResult, String> {
    require_settings_window(window.label())?;
    match super::rutracker::sign_in_with_credentials(window.app_handle(), &username, &password).await {
        Ok(logged_in_user) => {
            {
                let db = db.lock().await;
                let _ = db.set_app_setting(RUTRACKER_ENABLED_KEY, "true").await;
                let _ = db.set_app_setting(RUTRACKER_USERNAME_KEY, &logged_in_user).await;
            }
            let _ = window.emit_to("main", "torrent-indexers-changed", ());
            Ok(super::rutracker::RuTrackerSignInResult {
                signed_in: true,
                username: Some(logged_in_user),
                error: None,
                error_code: None,
                offer_browser_fallback: false,
            })
        }
        Err(failure) => Ok(super::rutracker::RuTrackerSignInResult {
            signed_in: false,
            username: None,
            error: Some(failure.message().into()),
            error_code: Some(failure.code()),
            offer_browser_fallback: failure.should_offer_browser(),
        }),
    }
}

#[tauri::command]
pub(crate) async fn sign_out_rutracker(
    window: WebviewWindow,
    db: State<'_, DbLock>,
) -> Result<(), String> {
    require_settings_window(window.label())?;
    super::rutracker::sign_out(window.app_handle()).await;
    {
        let db = db.lock().await;
        let _ = db.set_app_setting(RUTRACKER_ENABLED_KEY, "false").await;
        let _ = db.set_app_setting(RUTRACKER_USERNAME_KEY, "").await;
    }
    let _ = window.emit_to("main", "torrent-indexers-changed", ());
    Ok(())
}

#[tauri::command]
pub(crate) async fn is_rutracker_enabled(db: State<'_, DbLock>) -> Result<bool, String> {
    let db = db.lock().await;
    Ok(db
        .get_app_setting(RUTRACKER_ENABLED_KEY)
        .await
        .map_err(|_| "Could not load RuTracker settings.")?
        .as_deref()
        == Some("true"))
}

#[tauri::command]
pub(crate) async fn open_rutracker_login(window: WebviewWindow) -> Result<(), String> {
    require_settings_window(window.label())?;
    super::rutracker::open_login(window.app_handle()).await
}

#[derive(Default)]
pub(crate) struct Inspections(Mutex<InspectionJobs>);

#[derive(Default)]
struct InspectionJobs {
    pending: HashMap<Uuid, Option<oneshot::Sender<()>>>,
    cancelled_before_start: VecDeque<Uuid>,
}

impl Inspections {
    fn start(&self, id: Uuid) -> Result<oneshot::Receiver<()>, String> {
        let mut jobs = self
            .0
            .lock()
            .map_err(|_| "Inspection registry unavailable.")?;
        if let Some(index) = jobs
            .cancelled_before_start
            .iter()
            .position(|queued| *queued == id)
        {
            jobs.cancelled_before_start.remove(index);
            return Err("Torrent inspection cancelled.".into());
        }
        if jobs.pending.contains_key(&id) || jobs.pending.len() >= 8 {
            return Err("Too many inspections are pending.".into());
        }
        let (send, receive) = oneshot::channel();
        jobs.pending.insert(id, Some(send));
        Ok(receive)
    }

    fn cancel(&self, id: Uuid) {
        if let Ok(mut jobs) = self.0.lock() {
            if let Some(job) = jobs.pending.get_mut(&id) {
                if let Some(job) = job.take() {
                    let _ = job.send(());
                }
            } else if !jobs.cancelled_before_start.contains(&id) {
                if jobs.cancelled_before_start.len() >= 32 {
                    jobs.cancelled_before_start.pop_front();
                }
                jobs.cancelled_before_start.push_back(id);
            }
        }
    }
}

fn require_main_window(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Torrent inspection is available from the main window.".into())
    }
}

#[tauri::command]
pub(crate) async fn search_torrent_releases(
    window: WebviewWindow,
    db: State<'_, DbLock>,
    artist: String,
    title: String,
    progress: Channel<search::ProviderReport>,
) -> Result<search::SearchReport, String> {
    require_main_window(window.label())?;
    let enabled = enabled_indexers(&db).await?;
    search::search_with_app(Some(window.app_handle()), &artist, &title, false, &enabled, |report| {
        let _ = progress.send(report);
    })
    .await
}

#[tauri::command]
pub(crate) async fn inspect_torrent_release(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
    input: String,
) -> Result<inspect::InspectedRelease, String> {
    require_main_window(window.label())?;
    let cancelled = inspections.start(request_id)?;
    let result = tokio::select! {
        biased;
        _ = cancelled => Err("Torrent inspection cancelled.".into()),
        result = inspect::inspect_with_app(Some(window.app_handle()), &input) => result,
    };
    if let Ok(mut jobs) = inspections.0.lock() {
        jobs.pending.remove(&request_id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_scoped_search_can_try_a_track_title_without_repeating_the_album_query() {
        let track = |index, title: &str| matcher::ExpectedTrackInput {
            index, title: title.into(), artist: None, disc_number: None, track_number: None, duration_ms: None,
        };
        let selected = [
            track(4, "In Utero"), track(8, "All Apologies"), track(9, "X"),
        ];
        assert_eq!(alternate_track_query("in utero", &selected, &[]).unwrap().index, 8);
        assert!(alternate_track_query("In Utero", &selected[..1], &[]).is_none());
        assert!(alternate_track_query("In Utero", &[], &[]).is_none());
        assert!(alternate_track_query("In Utero", &vec![selected[1].clone(); 4], &[]).is_none());
    }

    #[test]
    fn an_early_cancel_does_not_start_an_inspection_or_consume_a_job_slot() {
        let inspections = Inspections::default();
        let cancelled = Uuid::new_v4();
        inspections.cancel(cancelled);
        assert!(inspections.start(cancelled).is_err());
        let jobs: Vec<_> = (0..8).map(|_| inspections.start(Uuid::new_v4())).collect();
        assert!(jobs.iter().all(Result::is_ok));
        assert!(inspections.start(Uuid::new_v4()).is_err());
    }

    #[tokio::test]
    async fn indexers_default_to_enabled_and_saved_toggles_control_search() {
        use search::Provider::{ApiBay, Archive, Knaben, Rutor, SolidTorrents};
        let db = DbLock::new(Database::new(":memory:").await);
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Archive, Knaben, ApiBay, SolidTorrents, Rutor]);
        {
            let db = db.lock().await;
            save_indexer(&db, Archive, false).await.unwrap();
            save_indexer(&db, Archive, false).await.unwrap();
            save_indexer(&db, ApiBay, false).await.unwrap();
            save_indexer(&db, Rutor, false).await.unwrap();
            let state = save_indexer(&db, SolidTorrents, false).await.unwrap();
            assert_eq!(disabled_indexers(&db).await.unwrap(), [Archive, ApiBay, Rutor, SolidTorrents]);
            assert_eq!(state.iter().filter(|indexer| indexer.enabled).count(), 1);
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Knaben]);
        {
            let db = db.lock().await;
            save_indexer(&db, Knaben, false).await.unwrap();
        }
        let enabled = enabled_indexers(&db).await.unwrap();
        let search = search::search_for_release("The Beatles", "Abbey Road", false, &enabled, |_| {
            panic!("disabled indexer was queried");
        }).await.unwrap();
        assert!(search.providers.is_empty());
        assert!(search.hits.is_empty());
        let (resolved, _) = matcher::resolve_with_indexers(None, "The Beatles", "Abbey Road", &[], &enabled, |_| {})
            .await.unwrap();
        assert!(resolved.is_empty());
        {
            let db = db.lock().await;
            save_indexer(&db, Archive, true).await.unwrap();
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), [Archive]);
    }

    #[tokio::test]
    async fn rutracker_is_opt_in_and_leaves_public_defaults_intact() {
        use search::Provider::RuTracker;
        let db = DbLock::new(Database::new(":memory:").await);
        assert!(!enabled_indexers(&db).await.unwrap().contains(&RuTracker));
        {
            let db = db.lock().await;
            let rows = save_indexer(&db, RuTracker, true).await.unwrap();
            assert!(rows.iter().find(|row| row.id == RuTracker).unwrap().enabled);
            assert!(rows.iter().all(|row| row.enabled));
        }
        assert!(enabled_indexers(&db).await.unwrap().contains(&RuTracker));
        {
            let db = db.lock().await;
            save_indexer(&db, RuTracker, false).await.unwrap();
        }
        assert_eq!(enabled_indexers(&db).await.unwrap(), search::Provider::ALL);
    }

    #[test]
    fn indexer_settings_and_torrent_search_use_separate_windows() {
        assert!(require_settings_window("settings").is_ok());
        assert!(require_settings_window("main").is_err());
        assert!(require_settings_window("youtube-login").is_err());
        assert!(require_main_window("settings").is_err());
    }
}

#[tauri::command]
pub(crate) async fn resolve_torrent_album(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
    artist: String,
    title: String,
    expected: Vec<matcher::ExpectedTrackInput>,
    progress: Channel<matcher::TorrentResolveEvent>,
) -> Result<(Vec<matcher::TorrentTierResolution>, Vec<String>), String> {
    require_main_window(window.label())?;
    let cancelled = inspections.start(request_id)?;
    let result = tokio::select! {
        biased;
        _ = cancelled => Err("Torrent resolution cancelled.".into()),
        res = resolve_enabled_torrents(window.app_handle(), &artist, &title, &expected, |event| {
            let _ = progress.send(event);
        }) => res,
    };
    if let Ok(mut jobs) = inspections.0.lock() {
        jobs.pending.remove(&request_id);
    }
    result
}

#[tauri::command]
pub(crate) fn cancel_torrent_inspection(
    window: WebviewWindow,
    inspections: State<'_, Inspections>,
    request_id: Uuid,
) -> Result<(), String> {
    require_main_window(window.label())?;
    inspections.cancel(request_id);
    Ok(())
}
