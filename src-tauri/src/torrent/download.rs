use crate::download::types::TrackDownload;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

const TORRENT_INIT_TIMEOUT: Duration = Duration::from_secs(25);
/// The quick request gives up this soon without data and asks again.
const TORRENT_QUICK_TIMEOUT: Duration = Duration::from_secs(20);
/// How long the patient request waits for anyone to be found.
const TORRENT_NO_PEERS_TIMEOUT: Duration = Duration::from_secs(30);
/// Peers were found, but none accepted a connection within this window.
const TORRENT_UNCONNECTED_PEERS_TIMEOUT: Duration = Duration::from_secs(15);
/// Peers are connected but nothing has arrived: restarting would only throw them away, so they get longer.
const TORRENT_SILENT_PEERS_TIMEOUT: Duration = Duration::from_secs(60);
const TORRENT_STALL_TIMEOUT: Duration = Duration::from_secs(45);
/// Longest wait for the next verified piece while unverified data keeps arriving.
const TORRENT_UNVERIFIED_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(350);
const NO_TORRENT_PEERS: &str = "No peers were found for the selected torrent.";
const ADD_TIMED_OUT: &str = "Timed out connecting to torrent peers or DHT.";
const INIT_TIMED_OUT: &str = "Timed out waiting for torrent peers/metadata.";

enum TransferError {
    NoPeers,
    /// The other request for the same torrent received data first.
    Superseded,
    Other(String),
}

impl TransferError {
    fn into_message(self) -> String {
        match self {
            Self::NoPeers => NO_TORRENT_PEERS.into(),
            Self::Superseded => "Another request for this torrent took over.".into(),
            Self::Other(message) => message,
        }
    }
}

/// Two requests for one torrent run side by side, each in its own session. The patient one keeps its peers
/// and the DHT; the quick one asks the trackers again and again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lane {
    Patient,
    Quick,
}

impl Lane {
    /// How long adding and initializing the torrent may take. The patient request gets longer, so the quick
    /// one has time to ask a second time before both are over.
    fn start_timeout(self) -> Duration {
        match self {
            Self::Patient => TORRENT_NO_PEERS_TIMEOUT,
            Self::Quick => TORRENT_INIT_TIMEOUT,
        }
    }
}

/// The quick request asks again after timing out while starting, as it does when nobody is found.
fn start_failure(lane: Lane, message: String) -> TransferError {
    if lane == Lane::Quick && (message == ADD_TIMED_OUT || message == INIT_TIMED_OUT) {
        TransferError::NoPeers
    } else {
        TransferError::Other(message)
    }
}

/// The request the torrent belongs to: the first one that receives a verified piece.
#[derive(Default)]
struct Winner(OnceLock<Lane>);

impl Winner {
    fn holder(&self) -> Option<Lane> {
        self.0.get().copied()
    }

    /// False when this request has to stop because the other one received data first.
    fn keeps(&self, lane: Lane, has_data: bool) -> bool {
        if has_data {
            *self.0.get_or_init(|| lane) == lane
        } else {
            self.holder().is_none_or(|holder| holder == lane)
        }
    }
}

/// Shared claim across multiple candidate torrents racing for the same download.
#[derive(Default)]
struct SwarmRaceWinner(OnceLock<usize>);

impl SwarmRaceWinner {
    fn holder(&self) -> Option<usize> {
        self.0.get().copied()
    }

    fn keeps(&self, candidate_idx: usize, has_data: bool) -> bool {
        if has_data {
            *self.0.get_or_init(|| candidate_idx) == candidate_idx
        } else {
            self.holder().is_none_or(|holder| holder == candidate_idx)
        }
    }
}

/// What the swarm of one torrent has shown so far.
#[derive(Debug, Clone, Copy, Default)]
struct SwarmSample {
    /// Bytes of pieces that passed the hash check.
    verified: u64,
    /// Bytes received from peers, including pieces that are still incomplete.
    fetched: u64,
    found: usize,
    connected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stall {
    NoPeers,
    SilentPeers,
    Interrupted,
}

struct SwarmWatch {
    lane: Lane,
    seen: SwarmSample,
    ever_connected: bool,
    last_data: Instant,
    last_verified: Instant,
}

impl SwarmWatch {
    fn new(now: Instant, lane: Lane) -> Self {
        Self {
            lane,
            seen: SwarmSample::default(),
            ever_connected: false,
            last_data: now,
            last_verified: now,
        }
    }

    fn observe(&mut self, sample: SwarmSample, now: Instant) {
        if sample.verified > self.seen.verified {
            self.last_verified = now;
            self.last_data = now;
        }
        if sample.fetched > self.seen.fetched {
            self.last_data = now;
        }
        if sample.connected > 0 {
            self.ever_connected = true;
        }
        self.seen = SwarmSample {
            verified: sample.verified.max(self.seen.verified),
            fetched: sample.fetched.max(self.seen.fetched),
            found: sample.found.max(self.seen.found),
            connected: sample.connected,
        };
    }

    fn has_data(&self) -> bool {
        self.seen.verified > 0 || self.seen.fetched > 0
    }

    /// Unverified bytes may come from a single peer that then goes silent, so they do not decide the winner.
    fn has_verified_data(&self) -> bool {
        self.seen.verified > 0
    }

    fn stall(&self, now: Instant) -> Option<Stall> {
        let idle = now.saturating_duration_since(self.last_data);
        if self.has_data() {
            let unverified = now.saturating_duration_since(self.last_verified);
            return (idle >= TORRENT_STALL_TIMEOUT || unverified >= TORRENT_UNVERIFIED_TIMEOUT)
                .then_some(Stall::Interrupted);
        }
        let (no_peers, silent_peers) = match self.lane {
            Lane::Patient => (TORRENT_NO_PEERS_TIMEOUT, TORRENT_SILENT_PEERS_TIMEOUT),
            Lane::Quick => (TORRENT_QUICK_TIMEOUT, TORRENT_QUICK_TIMEOUT),
        };
        if self.seen.found == 0 {
            (idle >= no_peers).then_some(Stall::NoPeers)
        } else if !self.ever_connected {
            let unconnected = silent_peers.min(TORRENT_UNCONNECTED_PEERS_TIMEOUT);
            (idle >= unconnected).then_some(Stall::SilentPeers)
        } else {
            (idle >= silent_peers).then_some(Stall::SilentPeers)
        }
    }

    fn waiting_message(&self, now: Instant) -> String {
        let idle = now.saturating_duration_since(self.last_data).as_secs();
        let SwarmSample { found, connected, .. } = self.seen;
        if self.has_data() {
            format!("Waiting for torrent data ({idle}s, connected peers: {connected})...")
        } else {
            format!("Waiting for torrent seeders ({idle}s, peers found: {found}, connected: {connected})...")
        }
    }
}

struct TorrentJob {
    dir: PathBuf,
    torrent: Option<(Arc<Session>, usize)>,
    cleaned: bool,
}

async fn remove_torrent_directory(dir: &std::path::Path) {
    for attempt in 0..8 {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) if attempt == 7 => {
                tracing::warn!(%error, "Failed cleaning torrent temporary directory");
                return;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

pub(crate) struct TorrentStagingCleanup(pub PathBuf);

impl TorrentStagingCleanup {
    /// Leaves the folder in place: its files are handed on to be saved.
    pub(crate) fn keep(mut self) {
        self.0 = PathBuf::new();
    }
}

impl Drop for TorrentStagingCleanup {
    fn drop(&mut self) {
        let dir = std::mem::take(&mut self.0);
        if dir.as_os_str().is_empty() { return; }
        tokio::spawn(async move { remove_torrent_directory(&dir).await; });
    }
}

impl TorrentJob {
    async fn clean(&mut self) {
        if let Some((session, id)) = &self.torrent {
            let _ = session.delete((*id).into(), true).await;
        }
        remove_torrent_directory(&self.dir).await;
        self.cleaned = true;
    }
}

impl Drop for TorrentJob {
    fn drop(&mut self) {
        if self.cleaned { return; }
        let mut cleanup = Self {
            dir: self.dir.clone(),
            torrent: self.torrent.take(),
            cleaned: false,
        };
        self.cleaned = true;
        tokio::spawn(async move { cleanup.clean().await; });
    }
}

impl From<String> for TransferError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

/// Starts a torrent once more when nobody was found, which announces it to the tracker again.
async fn retry_once_without_peers<T, F, Fut>(mut attempt: F) -> Result<T, TransferError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, TransferError>>,
{
    match attempt().await {
        Err(TransferError::NoPeers) => attempt().await,
        result => result,
    }
}

/// Runs the patient request once and repeats the quick one beside it while that finds nothing. The first
/// finished download wins; a request that fails without data leaves the other one to finish.
async fn race_lanes<T, P, Q, QuickFut>(winner: &Winner, patient: P, mut quick: Q) -> Result<T, String>
where
    P: std::future::Future<Output = Result<T, TransferError>>,
    Q: FnMut() -> QuickFut,
    QuickFut: std::future::Future<Output = Result<T, TransferError>>,
{
    use std::sync::atomic::{AtomicBool, Ordering};

    let patient_failed = AtomicBool::new(false);
    let repeated = async {
        loop {
            match quick().await {
                Err(TransferError::NoPeers) if !patient_failed.load(Ordering::SeqCst) => continue,
                result => return result,
            }
        }
    };
    let mut patient = std::pin::pin!(patient);
    let mut repeated = std::pin::pin!(repeated);
    let result = tokio::select! {
        result = &mut patient => match result {
            Ok(done) => Ok(done),
            Err(error) if winner.holder() == Some(Lane::Patient) => Err(error),
            Err(error) => {
                // The attempt already running may still succeed, but no new one is started.
                patient_failed.store(true, Ordering::SeqCst);
                match repeated.await {
                    Err(_) if winner.holder() != Some(Lane::Quick) => Err(error),
                    quick_result => quick_result,
                }
            }
        },
        result = &mut repeated => match result {
            Ok(done) => Ok(done),
            Err(error) if winner.holder() == Some(Lane::Quick) => Err(error),
            Err(_) => patient.await,
        },
    };
    result.map_err(TransferError::into_message)
}

#[derive(Debug, Clone)]
pub struct TorrentFileRequest {
    pub track_index: usize,
    pub file_index: usize,
    pub file_size: u64,
    pub track: TrackDownload,
}

#[derive(Debug, Clone)]
pub struct TorrentTransferResult {
    pub completed: Vec<(PathBuf, TrackDownload, usize)>,
    pub failed_tracks: Vec<(usize, TrackDownload)>,
    pub failure: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct SessionConfig {
    cache_root: PathBuf,
    disable_dht: bool,
}

static SESSIONS: OnceLock<Mutex<HashMap<SessionConfig, Arc<Session>>>> = OnceLock::new();

/// Forgets the sessions that use the DHT, so the next download starts a new one.
async fn reset_dht_session() {
    let mut sessions = SESSIONS.get_or_init(Default::default).lock().await;
    sessions.retain(|config, _| config.disable_dht);
}

async fn get_or_init_session(cache_root: PathBuf, disable_dht: bool) -> Result<Arc<Session>, String> {
    let desired = SessionConfig {
        cache_root: cache_root.clone(),
        disable_dht,
    };
    let mut sessions = SESSIONS.get_or_init(Default::default).lock().await;
    if let Some(session) = sessions.get(&desired) {
        return Ok(Arc::clone(session));
    }

    tokio::fs::create_dir_all(&cache_root)
        .await
        .map_err(|e| format!("Failed creating torrent cache dir: {e}"))?;

    let mut opts = SessionOptions::default();
    opts.disable_dht = disable_dht;
    opts.disable_dht_persistence = disable_dht;
    opts.fastresume = false;
    opts.persistence = None;
    opts.disable_upload = true;
    opts.listen_port_range = Some(49152..65535);
    opts.enable_upnp_port_forwarding = true;

    let session = match Session::new_with_opts(cache_root.clone(), opts).await {
        Ok(s) => s,
        Err(err) if !disable_dht => {
            tracing::warn!(error = %err, "Failed starting torrent session with DHT; retrying without DHT");
            let mut fallback_opts = SessionOptions::default();
            fallback_opts.disable_dht = true;
            fallback_opts.disable_dht_persistence = true;
            fallback_opts.fastresume = false;
            fallback_opts.persistence = None;
            fallback_opts.disable_upload = true;
            fallback_opts.listen_port_range = Some(49152..65535);
            fallback_opts.enable_upnp_port_forwarding = true;
            Session::new_with_opts(cache_root.clone(), fallback_opts)
                .await
                .map_err(|e| format!("Failed starting BitTorrent session: {e}"))?
        }
        Err(err) => return Err(format!("Failed starting BitTorrent session: {err}")),
    };
    sessions.insert(desired, Arc::clone(&session));
    Ok(session)
}

fn validate_relative_path(rel: &std::path::Path) -> Result<(), String> {
    use std::path::Component;
    if rel.is_absolute() || rel.as_os_str().is_empty() {
        return Err("Unsafe absolute torrent file path.".into());
    }
    for comp in rel.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return Err("Unsafe traversal component in torrent file path.".into()),
        }
    }
    Ok(())
}

const INITIAL_RACE_CANDIDATES: usize = 3;
const ALTERNATE_HEAD_START_SECS: u64 = 5;
const THIRD_CANDIDATE_DELAY_SECS: u64 = 8;

/// Downloads selected files from one torrent (`magnet_uri` or validated `.torrent` URL) using
/// `librqbit::Session`, emitting `download-track-progress` events and forwarding each completed
/// track to `track_sender` immediately so tagging & DB import happen pipelined.
pub async fn download_torrent_files(
    app: &AppHandle,
    task_id: &str,
    library_dir: &std::path::Path,
    locator: &str,
    requests: &[TorrentFileRequest],
    total_task_tracks: usize,
    completed_counter: Arc<std::sync::atomic::AtomicUsize>,
    track_sender: Option<tokio::sync::mpsc::Sender<(PathBuf, TrackDownload, usize)>>,
) -> Result<TorrentTransferResult, String> {
    let mut candidates = vec![(locator.to_owned(), requests.to_vec())];
    let mut attempted = HashSet::from([locator.to_owned()]);
    while candidates.len() < INITIAL_RACE_CANDIDATES {
        let Some((next, mapped)) = common_alternate(requests, &attempted) else {
            break;
        };
        attempted.insert(next.clone());
        candidates.push((next, mapped));
    }

    if candidates.len() > 1 && super::web_seed::archive_item(locator).is_none() {
        let race_winner = Arc::new(SwarmRaceWinner::default());
        let race_result = race_candidate_torrents(candidates.len(), |idx| {
            let app = app.clone();
            let task_id = task_id.to_owned();
            let library_dir = library_dir.to_path_buf();
            let (source, reqs) = candidates[idx].clone();
            let completed_counter = Arc::clone(&completed_counter);
            let track_sender = track_sender.clone();
            let race_winner = Arc::clone(&race_winner);
            async move {
                download_one_torrent(
                    &app, &task_id, &library_dir, &source, &reqs, total_task_tracks,
                    completed_counter, track_sender, Some((idx, race_winner)),
                ).await
            }
        }).await;
        match race_result {
            Ok(result) if result.failed_tracks.is_empty() && result.failure.is_none() => return Ok(result),
            Ok(result) => {
                let mut completed = result.completed;
                let failed: HashSet<_> = result.failed_tracks.iter().map(|(index, _)| *index).collect();
                let mut pending = requests.to_vec();
                pending.retain(|request| failed.contains(&request.track_index));
                if pending.is_empty() {
                    return Ok(TorrentTransferResult { completed, failed_tracks: Vec::new(), failure: None });
                }
                if let Some((next, mapped)) = common_alternate(&pending, &attempted) {
                    let tail = download_with_alternates(&next, &mapped, |source, reqs, retry| {
                        let completed_counter = Arc::clone(&completed_counter);
                        let track_sender = track_sender.clone();
                        async move {
                            if retry {
                                emit_task_progress(
                                    Some(app), task_id, completed_counter.load(std::sync::atomic::Ordering::SeqCst),
                                    total_task_tracks, "Retrying torrent with another release at the selected quality…",
                                ).await;
                            }
                            download_one_torrent(
                                app, task_id, library_dir, &source, &reqs, total_task_tracks,
                                completed_counter, track_sender, None,
                            ).await
                        }
                    }).await;
                    if let Ok(mut extra) = tail {
                        completed.extend(extra.completed);
                        extra.completed = completed;
                        return Ok(extra);
                    }
                }
                return Ok(TorrentTransferResult {
                    completed,
                    failure: result.failure,
                    failed_tracks: pending.into_iter().map(|req| (req.track_index, req.track)).collect(),
                });
            }
            Err(error) if error == "Task cancelled" => return Err(error),
            Err(primary_err) => {
                if let Some((next, mapped)) = common_alternate(requests, &attempted) {
                    return download_with_alternates(&next, &mapped, |source, reqs, _| {
                        let completed_counter = Arc::clone(&completed_counter);
                        let track_sender = track_sender.clone();
                        async move {
                            download_one_torrent(
                                app, task_id, library_dir, &source, &reqs, total_task_tracks,
                                completed_counter, track_sender, None,
                            ).await
                        }
                    }).await;
                }
                return Err(primary_err);
            }
        }
    }

    download_with_alternates(locator, requests, |source, pending, retry| {
        let completed_counter = Arc::clone(&completed_counter);
        let track_sender = track_sender.clone();
        async move {
            if retry {
                emit_task_progress(
                    Some(app), task_id, completed_counter.load(std::sync::atomic::Ordering::SeqCst),
                    total_task_tracks, "Retrying torrent with another release at the selected quality…",
                ).await;
            }
            download_one_torrent(
                app, task_id, library_dir, &source, &pending, total_task_tracks,
                completed_counter, track_sender, None,
            ).await
        }
    }).await
}

async fn race_candidate_torrents<F, Fut>(
    count: usize,
    mut run: F,
) -> Result<TorrentTransferResult, String>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Result<TorrentTransferResult, String>> + Send + 'static,
{
    use tokio::sync::Notify;
    let wake = Arc::new(Notify::new());
    let mut set = tokio::task::JoinSet::new();
    let futs: Vec<_> = (0..count).map(|idx| run(idx)).collect();
    let mut futs = futs.into_iter().enumerate();

    if let Some((idx, fut)) = futs.next() {
        set.spawn(async move { (idx, fut.await) });
    }
    for (idx, fut) in futs {
        let delay = if idx == 1 {
            Duration::from_secs(ALTERNATE_HEAD_START_SECS)
        } else {
            Duration::from_secs(THIRD_CANDIDATE_DELAY_SECS)
        };
        let wake_wait = Arc::clone(&wake);
        set.spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = wake_wait.notified() => {}
            }
            (idx, fut.await)
        });
    }

    let mut last_error: Option<String> = None;
    let mut partial_best: Option<TorrentTransferResult> = None;
    while let Some(joined) = set.join_next().await {
        let Ok((_idx, res)) = joined else { continue };
        match res {
            Ok(result) if result.failed_tracks.is_empty() && result.failure.is_none() => {
                set.abort_all();
                return Ok(result);
            }
            Ok(result) => {
                if !result.completed.is_empty() {
                    partial_best = Some(result);
                } else if let Some(err) = result.failure {
                    last_error = Some(err);
                }
                wake.notify_waiters();
            }
            Err(err) if err == "Task cancelled" => {
                set.abort_all();
                return Err(err);
            }
            Err(err) => {
                if err != "Another torrent source took over." {
                    last_error = Some(err);
                }
                wake.notify_waiters();
            }
        }
    }
    if let Some(best) = partial_best {
        Ok(best)
    } else {
        Err(last_error.unwrap_or_else(|| NO_TORRENT_PEERS.into()))
    }
}

async fn download_with_alternates<F, Fut>(
    locator: &str,
    requests: &[TorrentFileRequest],
    mut attempt: F,
) -> Result<TorrentTransferResult, String>
where
    F: FnMut(String, Vec<TorrentFileRequest>, bool) -> Fut,
    Fut: std::future::Future<Output = Result<TorrentTransferResult, String>>,
{
    let mut pending = requests.to_vec();
    let mut completed = Vec::new();
    let mut source = locator.to_owned();
    let mut attempted = HashSet::from([locator.to_owned()]);
    for retry in 0..=5 {
        let result = attempt(source.clone(), pending.clone(), retry > 0).await;
        let failure = match result {
            Ok(result) => {
                completed.extend(result.completed);
                let failed: HashSet<_> = result.failed_tracks.iter().map(|(index, _)| *index).collect();
                pending.retain(|request| failed.contains(&request.track_index));
                result.failure
            }
            Err(error) if error == "Task cancelled" => return Err(error),
            Err(error) => Some(error),
        };
        if pending.is_empty() {
            return Ok(TorrentTransferResult { completed, failed_tracks: Vec::new(), failure: None });
        }
        if retry < 5 {
            if let Some((next, requests)) = common_alternate(&pending, &attempted) {
                attempted.insert(next.clone());
                source = next;
                pending = requests;
                continue;
            }
        }
        if completed.is_empty() {
            if let Some(error) = failure {
                return Err(error);
            }
        }
        return Ok(TorrentTransferResult {
            completed, failure,
            failed_tracks: pending.into_iter().map(|request| (request.track_index, request.track)).collect(),
        });
    }
    unreachable!("the last bounded attempt returns")
}

fn common_alternate(
    requests: &[TorrentFileRequest],
    attempted: &HashSet<String>,
) -> Option<(String, Vec<TorrentFileRequest>)> {
    let first = requests.first()?.track.torrent_target.as_ref()?;
    first.alternatives.iter().find_map(|candidate| {
        if attempted.contains(&candidate.magnet_uri) || candidate.quality_tier != first.quality_tier {
            return None;
        }
        let mapped = requests.iter().map(|request| {
            let primary = request.track.torrent_target.as_ref()?;
            let replacement = primary.alternatives.iter().find(|alternative| {
                alternative.info_hash == candidate.info_hash
                    && alternative.magnet_uri == candidate.magnet_uri
                    && alternative.quality_tier == primary.quality_tier
            })?;
            Some(TorrentFileRequest {
                track_index: request.track_index,
                file_index: replacement.file_index,
                file_size: replacement.file_size,
                track: request.track.clone(),
            })
        }).collect::<Option<Vec<_>>>()?;
        Some((candidate.magnet_uri.clone(), mapped))
    })
}

async fn download_one_torrent(
    app: &AppHandle,
    task_id: &str,
    library_dir: &std::path::Path,
    locator: &str,
    requests: &[TorrentFileRequest],
    total_task_tracks: usize,
    completed_counter: Arc<std::sync::atomic::AtomicUsize>,
    track_sender: Option<tokio::sync::mpsc::Sender<(PathBuf, TrackDownload, usize)>>,
    race: Option<(usize, Arc<SwarmRaceWinner>)>,
) -> Result<TorrentTransferResult, String> {
    let mut archive_completed = Vec::new();
    let mut peer_requests = requests.to_vec();
    if let Some(item) = super::web_seed::archive_item(locator) {
        match download_archive_seed(
            Some(app), task_id, library_dir, locator, &item, requests, total_task_tracks,
            &completed_counter, track_sender.as_ref(),
        ).await {
            Ok(result) if result.failed_tracks.is_empty() && result.failure.is_none() => return Ok(result),
            Ok(result) => {
                let failed_indices: HashSet<usize> = result.failed_tracks.iter().map(|(idx, _)| *idx).collect();
                archive_completed = result.completed;
                peer_requests.retain(|req| failed_indices.contains(&req.track_index));
                tracing::debug!(
                    task_id,
                    completed = archive_completed.len(),
                    remaining = peer_requests.len(),
                    "Archive web seed partially completed; falling back to peers for remaining tracks"
                );
            }
            Err(error) if error == "Task cancelled" => return Err(error),
            Err(error) => tracing::debug!(task_id, %error, "Archive web seed unavailable; trying peers"),
        }
    }
    if peer_requests.is_empty() {
        return Ok(TorrentTransferResult {
            completed: archive_completed,
            failed_tracks: Vec::new(),
            failure: None,
        });
    }
    let winner = Winner::default();
    let attempt = |lane| {
        transfer_attempt(
            Some(app), task_id, library_dir, locator, &peer_requests, total_task_tracks,
            Arc::clone(&completed_counter), track_sender.clone(), lane, &winner,
            race.as_ref().map(|(idx, w)| (*idx, w.as_ref())),
        )
    };
    let mut peer_res = if super::rutracker::topic(locator).is_some() {
        // RuTracker torrents share one session, where a second request would only collide with the first.
        retry_once_without_peers(|| attempt(Lane::Patient)).await.map_err(TransferError::into_message)?
    } else {
        race_lanes(&winner, attempt(Lane::Patient), || attempt(Lane::Quick)).await?
    };
    if !archive_completed.is_empty() {
        archive_completed.extend(peer_res.completed);
        peer_res.completed = archive_completed;
    }
    Ok(peer_res)
}

async fn download_archive_seed(
    app: Option<&AppHandle>,
    task_id: &str,
    library_dir: &std::path::Path,
    locator: &str,
    item: &str,
    requests: &[TorrentFileRequest],
    total_tracks: usize,
    counter: &std::sync::atomic::AtomicUsize,
    sender: Option<&tokio::sync::mpsc::Sender<(PathBuf, TrackDownload, usize)>>,
) -> Result<TorrentTransferResult, String> {
    use std::sync::atomic::Ordering;
    if is_task_cancelled(app, task_id).await {
        return Err("Task cancelled".into());
    }
    let target = requests.first().and_then(|request| target_for_request(request, locator))
        .ok_or("Archive web seed requires a matched torrent.")?;
    if super::inspect::cached_torrent_bytes(locator).is_none() {
        super::inspect::inspect(locator).await?;
    }
    let bytes = super::inspect::cached_torrent_bytes(locator).ok_or("Missing Archive torrent metadata.")?;
    let inspected = super::inspect::parse_metainfo(&bytes, Some(&target.info_hash))?;
    for request in requests {
        let file = inspected.tracks.iter().find(|file| file.file_index == request.file_index);
        if !file.is_some_and(|file| selected_file_matches(request, locator, &file.file_path, file.size_bytes)) {
            return Err("The selected track no longer matches the torrent file list.".into());
        }
    }
    let seed = Arc::new(super::web_seed::ArchiveSeed::parse(&bytes, item, &target.info_hash)?);
    let staging = torrent_staging_dir(&torrent_cache_root(library_dir), task_id);
    std::fs::create_dir_all(&staging).map_err(|_| "Could not create web seed staging folder.")?;
    emit_task_progress(app, task_id, counter.load(Ordering::SeqCst), total_tracks, "Downloading from Archive web seed…").await;

    let concurrency = Arc::new(tokio::sync::Semaphore::new(3));
    let mut join_set = tokio::task::JoinSet::new();
    for request in requests {
        let permit = concurrency.clone();
        let seed = Arc::clone(&seed);
        let staging = staging.clone();
        let request = request.clone();
        let locator = locator.to_owned();
        let app_handle = app.cloned();
        let task_id = task_id.to_owned();
        let track_index = request.track_index;
        let task_id_outer = task_id.clone();
        let abort_handle = join_set.spawn(async move {
            let _guard = permit.acquire().await.map_err(|_| (request.track_index, request.track.clone(), "Task cancelled".to_string()))?;
            if is_task_cancelled(app_handle.as_ref(), &task_id).await {
                return Err((request.track_index, request.track.clone(), "Task cancelled".to_string()));
            }
            if is_track_cancelled(app_handle.as_ref(), &task_id, request.track_index).await {
                return Err((request.track_index, request.track.clone(), "Track cancelled".to_string()));
            }
            let source = request.track.torrent_target.as_ref()
                .and_then(|target| std::iter::once(target).chain(&target.alternatives).find(|source| source.magnet_uri == locator));
            let extension = source.and_then(|source| std::path::Path::new(&source.file_path).extension())
                .and_then(|ext| ext.to_str()).filter(|ext| ext.len() <= 8 && ext.bytes().all(|byte| byte.is_ascii_alphanumeric()))
                .unwrap_or("flac");
            let path = staging.join(format!("web-{}.{extension}", Uuid::new_v4()));
            let app_for_progress = app_handle.clone();
            let task_for_progress = task_id.clone();
            let track_index = request.track_index;
            let track_title = request.track.title.clone();
            let result = seed.transfer(request.file_index, request.file_size, &path, move |percent| {
                let app_p = app_for_progress.clone();
                let task_p = task_for_progress.clone();
                let title_p = track_title.clone();
                async move {
                    if is_track_cancelled(app_p.as_ref(), &task_p, track_index).await { return false; }
                    emit_track_progress(app_p.as_ref(), &task_p, track_index, &title_p, percent).await;
                    true
                }
            }).await;
            if let Some(registry) = app_handle.as_ref().and_then(|a| a.try_state::<crate::download::TaskRegistry>()) {
                registry.clear_track_abort_handle(&task_id, track_index).await;
            }
            match result {
                Ok(()) => {
                    if is_track_cancelled(app_handle.as_ref(), &task_id, request.track_index).await {
                        let _ = tokio::fs::remove_file(&path).await;
                        Err((request.track_index, request.track, "Track cancelled".to_string()))
                    } else {
                        Ok((path, request.track, request.track_index))
                    }
                }
                Err(error) => {
                    let _ = tokio::fs::remove_file(&path).await;
                    if is_track_cancelled(app_handle.as_ref(), &task_id, request.track_index).await {
                        Err((request.track_index, request.track, "Track cancelled".to_string()))
                    } else {
                        Err((request.track_index, request.track, error))
                    }
                }
            }
        });
        if let Some(registry) = app.and_then(|a| a.try_state::<crate::download::TaskRegistry>()) {
            registry.set_track_abort_handle(&task_id_outer, track_index, abort_handle).await;
        }
    }

    let mut completed: Vec<(PathBuf, TrackDownload, usize)> = Vec::new();
    let mut failed_tracks: Vec<(usize, TrackDownload)> = Vec::new();
    let mut first_error: Option<String> = None;
    let mut cancelled = false;
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(Ok((path, track, index))) => {
                emit_track_progress(app, task_id, index, &track.title, 100).await;
                let current = counter.fetch_add(1, Ordering::SeqCst) + 1;
                emit_task_progress(app, task_id, current, total_tracks, "Downloaded from Archive web seed").await;
                if let Some(sender) = sender {
                    let _ = sender.send((path.clone(), track.clone(), index)).await;
                }
                completed.push((path, track, index));
            }
            Ok(Err((_, _, error))) if error == "Task cancelled" => {
                cancelled = true;
                join_set.abort_all();
            }
            Ok(Err((_, _, error))) if error == "Track cancelled" => {}
            Ok(Err((index, track, error))) => {
                if !is_track_cancelled(app, task_id, index).await {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    failed_tracks.push((index, track));
                }
            }
            Err(join_err) => {
                if !join_err.is_cancelled() && first_error.is_none() {
                    first_error = Some(format!("Web seed worker failed: {join_err}"));
                }
            }
        }
    }
    if cancelled {
        for (staged, _, _) in completed {
            let _ = tokio::fs::remove_file(staged).await;
        }
        return Err("Task cancelled".into());
    }
    if completed.is_empty() {
        return Err(first_error.unwrap_or_else(|| "Archive web seed failed.".into()));
    }
    Ok(TorrentTransferResult {
        completed,
        failed_tracks,
        failure: first_error,
    })
}

fn selected_file_matches(request: &TorrentFileRequest, locator: &str, path: &str, size: u64) -> bool {
    if request.track.torrent_target.is_none() { return true; }
    let Some(expected) = target_for_request(request, locator) else { return false };
    let actual = path.replace('\\', "/");
    let expected_path = expected.file_path.replace('\\', "/");
    size == expected.file_size
        && (actual == expected_path || actual.strip_suffix(&expected_path).is_some_and(|prefix| prefix.ends_with('/')))
}

fn target_for_request<'a>(
    request: &'a TorrentFileRequest, locator: &str,
) -> Option<&'a crate::download::types::TorrentTarget> {
    let target = request.track.torrent_target.as_ref()?;
    std::iter::once(target).chain(&target.alternatives)
        .find(|item| item.magnet_uri == locator && item.file_index == request.file_index)
}

async fn transfer_attempt(
    app: Option<&AppHandle>,
    task_id: &str,
    library_dir: &std::path::Path,
    locator: &str,
    requests: &[TorrentFileRequest],
    total_task_tracks: usize,
    completed_counter: Arc<std::sync::atomic::AtomicUsize>,
    track_sender: Option<tokio::sync::mpsc::Sender<(PathBuf, TrackDownload, usize)>>,
    lane: Lane,
    winner: &Winner,
    race: Option<(usize, &SwarmRaceWinner)>,
) -> Result<TorrentTransferResult, TransferError> {
    use std::sync::atomic::Ordering;

    if requests.is_empty() {
        return Ok(TorrentTransferResult {
            completed: Vec::new(),
            failed_tracks: Vec::new(),
            failure: None,
        });
    }

    if is_task_cancelled(app, task_id).await {
        return Err("Task cancelled".to_string().into());
    }
    let rutracker_bytes = if let Some(topic) = super::rutracker::topic(locator) {
        let app = app.ok_or("RuTracker browser session is unavailable.".to_string())?;
        let expected = requests.first().and_then(|request| target_for_request(request, locator))
            .ok_or("RuTracker track has no verified torrent metadata.".to_string())?;
        Some(super::rutracker::torrent(app, topic, Some(&expected.info_hash)).await?.1)
    } else {
        // Validate scheme before the locator is used.
        super::inspect::normalize_locator_for_download(locator)?;
        None
    };

    let temp_root = library_dir.join(".temp");
    let _ = tokio::fs::create_dir_all(&temp_root).await;
    let _ = crate::utils::set_hidden(&temp_root, true);
    let cache_root = torrent_cache_root(library_dir);
    let job_dir = cache_root.join(format!("job-{}", Uuid::new_v4()));
    let mut job = TorrentJob { dir: job_dir.clone(), torrent: None, cleaned: false };
    std::fs::create_dir_all(&job_dir)
        .map_err(|e| format!("Failed creating torrent job folder: {e}"))?;
    let staging_dir = torrent_staging_dir(&cache_root, task_id);
    let _ = std::fs::create_dir_all(&staging_dir);

    let mut only_files: Vec<usize> = requests.iter().map(|r| r.file_index).collect();
    only_files.sort_unstable();
    only_files.dedup();

    let added = if let Some(bytes) = rutracker_bytes {
        match get_or_init_session(cache_root.join("rutracker"), false).await {
            Ok(session) => try_add_torrent(
                app, task_id, &session, locator, Some(bytes), &only_files, &job_dir,
                total_task_tracks, Some("Connecting to RuTracker peers"), lane.start_timeout(),
            ).await.map(|handle| (session, handle)),
            Err(error) => Err(error),
        }
    } else {
        add_torrent(
            app, task_id, &cache_root, &job_dir, locator, &only_files, total_task_tracks, lane,
        )
        .await
    };
    let (session, handle) = match added {
        Ok(added) => added,
        Err(error) => {
            job.clean().await;
            return Err(start_failure(lane, error));
        }
    };
    job.torrent = Some((Arc::clone(&session), handle.id()));
    let hash = handle.info_hash().as_string();
    if requests.iter().filter_map(|request| target_for_request(request, locator))
        .any(|target| !target.info_hash.eq_ignore_ascii_case(&hash))
    {
        job.clean().await;
        return Err("Torrent metadata has changed since the tracks were matched.".to_string().into());
    }

    let init_started = Instant::now();
    let mut last_status_at = Instant::now();
    let init_res: Result<(), String> = loop {
        if is_task_cancelled(app, task_id).await {
            break Err("Task cancelled".to_string());
        }
        if let Ok(Ok(())) = tokio::time::timeout(
            Duration::from_millis(250),
            handle.wait_until_initialized(),
        )
        .await
        {
            break Ok(());
        }
        let elapsed = init_started.elapsed();
        if elapsed >= lane.start_timeout() {
            break Err(INIT_TIMED_OUT.to_string());
        }
        if lane == Lane::Patient && last_status_at.elapsed() >= Duration::from_secs(2) {
            last_status_at = Instant::now();
            emit_task_progress(
                app,
                task_id,
                completed_counter.load(Ordering::SeqCst),
                total_task_tracks,
                &format!("Waiting for torrent metadata ({}s)…", elapsed.as_secs()),
            )
            .await;
        }
    };

    if let Err(msg) = init_res {
        job.clean().await;
        return Err(start_failure(lane, msg));
    }

    if let Some(registry) = app.and_then(|a| a.try_state::<crate::download::TaskRegistry>()) {
        let track_to_file: HashMap<usize, usize> = requests
            .iter()
            .map(|r| (r.track_index, r.file_index))
            .collect();
        registry
            .register_torrent_selection(
                task_id,
                Arc::clone(&session),
                Arc::clone(&handle),
                track_to_file,
            )
            .await;
    }

    if lane == Lane::Patient {
        emit_task_progress(
            app,
            task_id,
            completed_counter.load(Ordering::SeqCst),
            total_task_tracks,
            "Waiting for torrent seeders…",
        )
        .await;
    }

    let file_rel_paths: HashMap<usize, (PathBuf, u64)> = match handle.with_metadata(|meta| {
        requests
            .iter()
            .filter_map(|req| {
                let fi = meta.file_infos.get(req.file_index)?;
                Some((req.file_index, (fi.relative_filename.clone(), fi.len)))
            })
            .collect()
    }) {
        Ok(map) => map,
        Err(e) => {
            job.clean().await;
            return Err(format!("Torrent metadata unavailable: {e}").into());
        }
    };

    for (rel, _) in file_rel_paths.values() {
        if let Err(err) = validate_relative_path(rel) {
            job.clean().await;
            return Err(err.into());
        }
    }

    let mut done_indices = HashSet::<usize>::new();
    let mut last_pct: HashMap<usize, i32> = HashMap::new();
    let mut completed = Vec::new();
    let mut failed_tracks = Vec::new();
    let mut watch = SwarmWatch::new(Instant::now(), lane);
    let mut last_wait_notice_at = Instant::now();
    let mut cancelled = false;
    let mut transfer_failure: Option<String> = None;
    let mut no_peers = false;
    let mut superseded = false;

    loop {
        if is_task_cancelled(app, task_id).await {
            cancelled = true;
            break;
        }

        let stats = handle.stats();
        if let Some(err) = &stats.error {
            tracing::warn!(task_id, error = %err, "Torrent transfer reported error");
            transfer_failure = Some(format!("Torrent transfer error: {err}"));
            break;
        }

        let live = stats.live.as_ref().map(|live| &live.snapshot);
        let now = Instant::now();
        let verified_before = watch.seen.verified;
        watch.observe(SwarmSample {
            verified: stats.progress_bytes,
            fetched: live.map_or(0, |live| live.fetched_bytes),
            found: live.map_or(0, |live| live.peer_stats.seen),
            connected: live.map_or(0, |live| live.peer_stats.live),
        }, now);
        // Checked before any file is staged, so only one request ever hands tracks on.
        let undecided = winner.holder().is_none();
        let has_verified = watch.has_verified_data() || stats.finished;
        if !winner.keeps(lane, has_verified)
            || race.is_some_and(|(idx, swarm_winner)| !swarm_winner.keeps(idx, has_verified))
        {
            superseded = true;
            break;
        }
        if undecided && winner.holder().is_some() {
            let SwarmSample { found, connected, .. } = watch.seen;
            tracing::info!(task_id, ?lane, found, connected, "Torrent data arrived");
        }
        if !stats.finished {
            if let Some(stall) = watch.stall(now) {
                let SwarmSample { verified, fetched, found, connected } = watch.seen;
                tracing::warn!(task_id, ?lane, ?stall, found, connected, fetched, verified, "Torrent download stalled");
                match stall {
                    Stall::NoPeers => no_peers = true,
                    // The quick request simply asks again.
                    Stall::SilentPeers if lane == Lane::Quick => no_peers = true,
                    Stall::Interrupted if lane == Lane::Quick && !watch.has_verified_data() => no_peers = true,
                    Stall::SilentPeers => transfer_failure = Some(
                        "Peers were found for the selected torrent, but none sent data.".into(),
                    ),
                    Stall::Interrupted => transfer_failure = Some(
                        "Torrent download stalled while waiting for remaining data.".into(),
                    ),
                }
                break;
            }
            let reports_lane = winner.holder().unwrap_or(Lane::Patient) == lane;
            let reports_swarm = race.is_none_or(|(idx, swarm_winner)| swarm_winner.holder().unwrap_or(0) == idx);
            if reports_lane && reports_swarm && watch.seen.verified == verified_before && last_wait_notice_at.elapsed() >= Duration::from_secs(3) {
                last_wait_notice_at = now;
                emit_task_progress(
                    app,
                    task_id,
                    completed_counter.load(Ordering::SeqCst),
                    total_task_tracks,
                    &watch.waiting_message(now),
                )
                .await;
            }
        }

        for req in requests {
            if done_indices.contains(&req.track_index) {
                continue;
            }
            if is_track_cancelled(app, task_id, req.track_index).await {
                done_indices.insert(req.track_index);
                continue;
            }
            let Some((rel_path, meta_len)) = file_rel_paths.get(&req.file_index) else {
                done_indices.insert(req.track_index);
                failed_tracks.push((req.track_index, req.track.clone()));
                continue;
            };
            if !selected_file_matches(req, locator, &rel_path.to_string_lossy(), *meta_len) {
                done_indices.insert(req.track_index);
                failed_tracks.push((req.track_index, req.track.clone()));
                transfer_failure = Some("The selected track no longer matches the torrent file list.".into());
                continue;
            }

            let target_bytes = (*meta_len).max(req.file_size);
            let done_bytes = stats.file_progress.get(req.file_index).copied().unwrap_or(0);
            let is_complete =
                stats.finished || (target_bytes > 0 && done_bytes >= target_bytes);

            if is_complete {
                let src_path = job_dir.join(rel_path);
                let ext = src_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("flac");
                let staged = staging_dir.join(format!(
                    "track_{}_{}.{}",
                    req.track_index,
                    Uuid::new_v4().simple(),
                    ext
                ));
                if let Err(e) = copy_or_move_file(&src_path, &staged).await {
                    tracing::warn!(track = %req.track.title, error = %e, "Failed staging completed torrent track");
                    done_indices.insert(req.track_index);
                    failed_tracks.push((req.track_index, req.track.clone()));
                    continue;
                }

                done_indices.insert(req.track_index);
                emit_track_progress(app, task_id, req.track_index, &req.track.title, 100).await;
                let cur = completed_counter.fetch_add(1, Ordering::SeqCst) + 1;
                emit_task_progress(
                    app,
                    task_id,
                    cur,
                    total_task_tracks,
                    &format!("Downloaded {} of {} tracks", cur, total_task_tracks),
                )
                .await;

                if let Some(ref sender) = track_sender {
                    let _ = sender
                        .send((staged.clone(), req.track.clone(), req.track_index))
                        .await;
                }
                completed.push((staged, req.track.clone(), req.track_index));
            } else if target_bytes > 0 {
                let pct = ((done_bytes.saturating_mul(100)) / target_bytes).min(99) as i32;
                let prev = last_pct.get(&req.track_index).copied().unwrap_or(-1);
                if pct > prev {
                    last_pct.insert(req.track_index, pct);
                    emit_track_progress(app, task_id, req.track_index, &req.track.title, pct).await;
                }
            }
        }

        if done_indices.len() >= requests.len() {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    for req in requests {
        if !done_indices.contains(&req.track_index) {
            failed_tracks.push((req.track_index, req.track.clone()));
        }
    }

    job.clean().await;

    if cancelled {
        return Err("Task cancelled".to_string().into());
    }
    if superseded {
        return Err(TransferError::Superseded);
    }
    if no_peers {
        return Err(TransferError::NoPeers);
    }
    if completed.is_empty() {
        if let Some(ref err) = transfer_failure {
            return Err(err.clone().into());
        }
    }

    Ok(TorrentTransferResult {
        completed,
        failed_tracks,
        failure: transfer_failure,
    })
}

async fn add_torrent(
    app: Option<&AppHandle>,
    task_id: &str,
    cache_root: &std::path::Path,
    job_dir: &std::path::Path,
    locator: &str,
    only_files: &[usize],
    total_task_tracks: usize,
    lane: Lane,
) -> Result<(Arc<Session>, Arc<ManagedTorrent>), String> {
    let status = (lane == Lane::Patient).then_some("Connecting to torrent peers");
    if let Some(status) = status {
        emit_task_progress(app, task_id, 0, total_task_tracks, &format!("{status}…")).await;
    }
    let session = get_or_init_session(cache_root.to_path_buf(), lane == Lane::Quick).await?;
    let added = try_add_torrent(
        app, task_id, &session, locator, None, only_files, job_dir, total_task_tracks, status,
        lane.start_timeout(),
    )
    .await;
    match added {
        Ok(handle) => Ok((session, handle)),
        Err(ref error) if error == ALREADY_MANAGED_ERR => {
            let isolated_dir = job_dir.join(".session");
            tokio::fs::create_dir_all(&isolated_dir)
                .await
                .map_err(|e| format!("Failed creating isolated torrent session dir: {e}"))?;
            let mut opts = SessionOptions::default();
            opts.disable_dht = lane == Lane::Quick;
            opts.disable_dht_persistence = true;
            opts.fastresume = false;
            opts.persistence = None;
            opts.disable_upload = true;
            let isolated = Session::new_with_opts(isolated_dir, opts)
                .await
                .map_err(|e| format!("Failed starting isolated BitTorrent session: {e}"))?;
            let handle = try_add_torrent(
                app, task_id, &isolated, locator, None, only_files, job_dir, total_task_tracks, status,
                lane.start_timeout(),
            )
            .await?;
            Ok((isolated, handle))
        }
        Err(error) => {
            if error != "Task cancelled" {
                tracing::warn!(task_id, ?lane, %error, "Failed adding the torrent");
                if lane == Lane::Patient {
                    // librqbit's DHT UDP actor can exit (e.g. WSAEMSGSIZE 10040 on Windows): start it anew.
                    reset_dht_session().await;
                }
            }
            Err(error)
        }
    }
}

/// The only source of what librqbit adds from a locator. It must never receive an HTTP(S) URL: librqbit
/// would fetch it without the public-address and size checks of `inspect`.
async fn torrent_source(locator: &str) -> Result<AddTorrent<'static>, String> {
    super::inspect::normalize_locator_for_download(locator)?;
    if let Some(bytes) = super::inspect::cached_torrent_bytes(locator) {
        return Ok(AddTorrent::from_bytes(bytes));
    }
    let locator = locator.trim();
    if locator.starts_with("magnet:") {
        return Ok(AddTorrent::from_url(locator.to_owned()));
    }
    super::inspect::inspect(locator).await?;
    super::inspect::cached_torrent_bytes(locator)
        .map(AddTorrent::from_bytes)
        .ok_or_else(|| "Torrent metadata is unavailable.".into())
}

const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
];

const ALREADY_MANAGED_ERR: &str = "This torrent is already downloading in this session.";

async fn try_add_torrent(
    app: Option<&AppHandle>,
    task_id: &str,
    session: &Arc<Session>,
    locator: &str,
    verified_bytes: Option<Vec<u8>>,
    only_files: &[usize],
    job_dir: &std::path::Path,
    total_task_tracks: usize,
    status_prefix: Option<&str>,
    timeout: Duration,
) -> Result<Arc<ManagedTorrent>, String> {
    let add_opts = AddTorrentOptions {
        paused: false,
        only_files: Some(only_files.to_vec()),
        overwrite: true,
        output_folder: Some(job_dir.to_string_lossy().to_string()),
        trackers: Some(PUBLIC_TRACKERS.iter().map(|t| (*t).to_string()).collect()),
        ..Default::default()
    };

    let started = Instant::now();
    let mut last_notice = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut add_fut = std::pin::pin!(async {
        let source = match verified_bytes {
            Some(bytes) => AddTorrent::from_bytes(bytes),
            None => torrent_source(locator).await?,
        };
        session.add_torrent(source, Some(add_opts)).await
            .map_err(|e| format!("Failed starting torrent download: {e}"))
    });

    let response = loop {
        tokio::select! {
            res = &mut add_fut => {
                break res?;
            }
            _ = tick.tick() => {
                if is_task_cancelled(app, task_id).await {
                    return Err("Task cancelled".into());
                }
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    return Err(ADD_TIMED_OUT.into());
                }
                if let Some(status_prefix) = status_prefix.filter(|_| last_notice.elapsed() >= Duration::from_secs(2)) {
                    last_notice = Instant::now();
                    emit_task_progress(
                        app,
                        task_id,
                        0,
                        total_task_tracks,
                        &format!("{status_prefix} ({}s)…", elapsed.as_secs()),
                    )
                    .await;
                }
            }
        }
    };

    match response {
        AddTorrentResponse::Added(_, handle) => Ok(handle),
        AddTorrentResponse::AlreadyManaged(_, _) => {
            Err(ALREADY_MANAGED_ERR.into())
        }
        AddTorrentResponse::ListOnly(_) => {
            Err("Torrent metadata list-only returned unexpectedly.".into())
        }
    }
}

async fn emit_task_progress(
    app: Option<&AppHandle>,
    task_id: &str,
    current: usize,
    total: usize,
    message: &str,
) {
    let Some(app) = app else { return };
    let _ = app.emit(
        "download-task-progress",
        crate::download::types::ProgressEvent {
            task_id: task_id.to_string(),
            current,
            total,
            track_title: message.to_string(),
        },
    );
    if let Some(ref registry) = app.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_task_progress(task_id, current, total, message)
            .await;
    }
}

async fn is_task_cancelled(app: Option<&AppHandle>, task_id: &str) -> bool {
    if let Some(registry) = app.and_then(|app| app.try_state::<crate::download::TaskRegistry>()) {
        registry.is_cancelled(task_id).await
    } else {
        false
    }
}

async fn is_track_cancelled(app: Option<&AppHandle>, task_id: &str, track_index: usize) -> bool {
    if let Some(registry) = app.and_then(|app| app.try_state::<crate::download::TaskRegistry>()) {
        registry.is_track_cancelled(task_id, track_index).await
    } else {
        false
    }
}

async fn emit_track_progress(
    app: Option<&AppHandle>,
    task_id: &str,
    track_index: usize,
    title: &str,
    percent: i32,
) {
    let Some(app) = app else { return };
    let _ = app.emit(
        "download-track-progress",
        serde_json::json!({
            "task_id": task_id,
            "track_index": track_index,
            "track_title": title,
            "percent": percent,
        }),
    );
    if let Some(registry) = app.try_state::<crate::download::TaskRegistry>() {
        registry
            .update_track_progress(task_id, track_index, title, percent)
            .await;
    }
}

async fn copy_or_move_file(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if tokio::fs::rename(src, dst).await.is_ok() {
        return Ok(());
    }
    tokio::fs::copy(src, dst)
        .await
        .map(|_| ())
        .map_err(|e| format!("Failed copying staged torrent audio file: {e}"))
}

pub(crate) fn torrent_cache_root(library_dir: &std::path::Path) -> PathBuf {
    library_dir.join(".temp").join("torrents")
}

pub(crate) fn torrent_staging_dir(cache_root: &std::path::Path, task_id: &str) -> PathBuf {
    let clean: String = task_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    cache_root.join(format!("staged-{}", clean))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_mismatched_nevermind_file_at_the_selected_index() {
        let locator = "magnet:?xt=urn:btih:test";
        let request = TorrentFileRequest {
            track_index: 12, file_index: 6, file_size: 60,
            track: TrackDownload {
                title: "Endless, Nameless".into(),
                torrent_target: Some(crate::download::types::TorrentTarget {
                    info_hash: "test".into(), magnet_uri: locator.into(), file_index: 6,
                    file_path: "13 Endless, Nameless.flac".into(), file_size: 60,
                    quality_tier: crate::metadata::types::AudioQualityTier::Lossless,
                    alternatives: Vec::new(),
                }),
                ..Default::default()
            },
        };
        assert!(!selected_file_matches(&request, locator, "07 Territorial Pissings.flac", 60));
        assert!(selected_file_matches(&request, locator, "Nevermind/13 Endless, Nameless.flac", 60));
    }
    use std::path::Path;

    #[tokio::test]
    #[ignore = "downloads the real Syro release into an isolated temporary folder"]
    async fn live_syro_resolution_and_transfer() {
        use crate::metadata::types::AudioQualityTier;
        use crate::torrent::matcher::{resolve_album_torrents, ExpectedTrackInput};

        let titles = [
            "minipops 67 [120.2][source field mix]", "XMAS_EVET10 [120][thanaton3 mix]",
            "produk 29 [101]", "4 bit 9d api+e+6 [126.26]", "180db_ [130]",
            "CIRCLONT6A [141.98][syrobonkus mix]", "fz pseudotimestretch+e+3 [138.85]",
            "CIRCLONT14 [152.97][shrymoming mix]", "syro u473t8+e [141.98][piezoluminescence mix]",
            "PAPAT4 [155][pineal mix]", "s950tx16wasr10 [163.97][earth portal mix]", "aisatsana [102]",
        ];
        let expected = titles.iter().enumerate().map(|(index, title)| ExpectedTrackInput {
            index, title: (*title).into(), artist: Some("Aphex Twin".into()),
            disc_number: Some(1), track_number: Some(index as u32 + 1), duration_ms: None,
        }).collect::<Vec<_>>();
        let (releases, unavailable) = resolve_album_torrents("Aphex Twin", "Syro", &expected, |_| {}).await.unwrap();
        println!("Syro: {} resolved releases, unavailable providers: {unavailable:?}", releases.len());
        let release = releases.iter().find(|release| release.tier == AudioQualityTier::Lossless)
            .expect("Syro should resolve to a lossless source");
        assert_eq!(release.matched_count, expected.len());
        let item = super::super::web_seed::archive_item(&release.magnet_uri)
            .expect("a complete web seed should rank ahead of the peer-only release");
        let requests: Vec<_> = release.tracks.iter().map(|matched| {
            let mut track = TrackDownload { title: titles[matched.expected_index].into(), ..Default::default() };
            crate::download::album::apply_candidate_to_track(
                &mut track, std::slice::from_ref(&matched.candidate), Some("source:torrent:lossless"), 0,
            );
            let target = track.torrent_target.as_ref().expect("resolver should stage a torrent candidate");
            TorrentFileRequest {
                track_index: matched.expected_index, file_index: target.file_index,
                file_size: target.file_size, track,
            }
        }).collect();
        let temp = tempfile::tempdir().unwrap();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(expected.len());
        let result = download_archive_seed(
            None, "live-syro", temp.path(), &release.magnet_uri, &item, &requests,
            expected.len(), &counter, Some(&sender),
        ).await.unwrap();
        assert!(result.failed_tracks.is_empty());
        assert_eq!(result.completed.len(), expected.len());
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), expected.len());
        let mut total_bytes = 0;
        for _ in &expected {
            let (path, _, index) = receiver.try_recv().unwrap();
            let request = requests.iter().find(|request| request.track_index == index).unwrap();
            assert_eq!(tokio::fs::metadata(&path).await.unwrap().len(), request.file_size);
            let mut file = tokio::fs::File::open(path).await.unwrap();
            let mut signature = [0; 4];
            tokio::io::AsyncReadExt::read_exact(&mut file, &mut signature).await.unwrap();
            assert_eq!(&signature, b"fLaC");
            total_bytes += request.file_size;
        }
        println!("Syro: staged {} verified FLAC tracks, {total_bytes} bytes, Archive item {item}", expected.len());
    }

    #[tokio::test]
    async fn alternate_release_requires_the_same_quality_and_all_pending_album_tracks() {
        use crate::download::types::TorrentTarget;
        use crate::metadata::types::AudioQualityTier::{HighLossy, Lossless};

        let target = |hash: &str, file_index, quality_tier| TorrentTarget {
            info_hash: hash.to_owned(),
            magnet_uri: format!("magnet:{hash}"),
            file_path: format!("{file_index}.flac"),
            file_index,
            file_size: 100 + file_index as u64,
            quality_tier,
            alternatives: Vec::new(),
        };
        let request = |index, alternatives| TorrentFileRequest {
            track_index: index,
            file_index: index,
            file_size: 100,
            track: TrackDownload {
                torrent_target: Some(TorrentTarget { alternatives, ..target("primary", index, Lossless) }),
                ..Default::default()
            },
        };
        let first = request(0, vec![target("lossy", 1, HighLossy), target("partial", 2, Lossless), target("complete", 3, Lossless)]);
        let second = request(1, vec![target("complete", 8, Lossless)]);
        let (_, mapped) = common_alternate(&[first.clone(), second.clone()], &HashSet::new()).unwrap();
        assert_eq!(mapped.iter().map(|r| (r.track_index, r.file_index, r.file_size)).collect::<Vec<_>>(), [(0, 3, 103), (1, 8, 108)]);
        let attempted = HashSet::from(["magnet:complete".to_owned()]);
        assert!(common_alternate(&[first.clone(), second], &attempted).is_none());
        let (source, _) = common_alternate(&[first.clone()], &attempted).unwrap();
        assert_eq!(source, "magnet:partial");

        let mut attempts = Vec::new();
        let result = download_with_alternates("magnet:primary", &[first], |source, mut pending, retry| {
            attempts.push(source);
            let request = pending.pop().unwrap();
            std::future::ready(if retry {
                assert_eq!(request.file_index, 2);
                Ok(TorrentTransferResult {
                    completed: vec![(PathBuf::from("recovered.flac"), request.track, request.track_index)],
                    failed_tracks: Vec::new(), failure: None,
                })
            } else {
                Err("No peers responded".into())
            })
        }).await.unwrap();
        assert_eq!(attempts, ["magnet:primary", "magnet:partial"]);
        assert_eq!(result.completed.len(), 1);
        assert!(result.failed_tracks.is_empty());
    }

    async fn after<T>(milliseconds: u64, value: T) -> T {
        tokio::time::sleep(Duration::from_millis(milliseconds)).await;
        value
    }

    #[tokio::test]
    async fn the_quick_request_asks_again_while_the_patient_one_waits() {
        let mut attempts = 0;
        let result = race_lanes(&Winner::default(), std::future::pending(), || {
            attempts += 1;
            after(5, if attempts == 3 { Ok("quick") } else { Err(TransferError::NoPeers) })
        }).await;
        assert_eq!(result.unwrap(), "quick");
        assert_eq!(attempts, 3);
    }

    #[tokio::test]
    async fn a_running_quick_attempt_may_still_succeed_after_the_patient_one_failed() {
        let result = race_lanes(
            &Winner::default(),
            after(5, Err(TransferError::NoPeers)),
            || after(40, Ok("quick")),
        ).await;
        assert_eq!(result.unwrap(), "quick");
    }

    #[tokio::test]
    async fn no_new_quick_attempt_starts_once_the_patient_one_failed() {
        let mut attempts = 0;
        let result = race_lanes(
            &Winner::default(),
            after(5, Err::<(), _>(TransferError::Other("Task cancelled".into()))),
            || {
                attempts += 1;
                after(40, Err(TransferError::NoPeers))
            },
        ).await;
        assert_eq!(result.unwrap_err(), "Task cancelled");
        assert_eq!(attempts, 1);
    }

    #[tokio::test]
    async fn a_failed_quick_request_leaves_the_patient_one_to_finish() {
        let result = race_lanes(
            &Winner::default(),
            after(40, Ok("patient")),
            || after(5, Err(TransferError::Other("no trackers".into()))),
        ).await;
        assert_eq!(result.unwrap(), "patient");
    }

    #[tokio::test]
    async fn the_request_that_received_data_decides_the_outcome() {
        let winner = Winner::default();
        assert!(winner.keeps(Lane::Quick, true));
        assert!(!winner.keeps(Lane::Patient, false));
        assert!(!winner.keeps(Lane::Patient, true));
        let result = race_lanes(
            &winner,
            after(5, Err::<(), _>(TransferError::Superseded)),
            || after(40, Err(TransferError::Other("stalled".into()))),
        ).await;
        assert_eq!(result.unwrap_err(), "stalled");

        let winner = Winner::default();
        assert!(winner.keeps(Lane::Patient, true));
        let result = race_lanes(
            &winner,
            after(5, Err::<(), _>(TransferError::Other("interrupted".into()))),
            std::future::pending,
        ).await;
        assert_eq!(result.unwrap_err(), "interrupted");
    }

    #[test]
    fn the_quick_request_gives_up_sooner_than_the_patient_one() {
        let start = Instant::now();
        let mut quick = SwarmWatch::new(start, Lane::Quick);
        assert_eq!(quick.stall(start + TORRENT_QUICK_TIMEOUT), Some(Stall::NoPeers));
        quick.observe(SwarmSample { found: 3, ..Default::default() }, start);
        assert_eq!(quick.stall(start + TORRENT_QUICK_TIMEOUT), Some(Stall::SilentPeers));
        let patient = SwarmWatch::new(start, Lane::Patient);
        assert_eq!(patient.stall(start + TORRENT_QUICK_TIMEOUT), None);
    }

    #[tokio::test]
    async fn aborted_job_removes_managed_torrent_and_its_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("job-cancel");
        let staging = root.path().join("staged-cancel");
        std::fs::create_dir(&dir).unwrap();
        std::fs::create_dir(&staging).unwrap();
        let session = Session::new_with_opts(root.path().to_owned(), SessionOptions {
            disable_dht: true,
            disable_upload: true,
            ..Default::default()
        }).await.unwrap();
        let mut metadata = b"d4:infod6:lengthi3e4:name9:song.flac12:piece lengthi16384e6:pieces20:".to_vec();
        metadata.extend_from_slice(&[0; 20]);
        metadata.extend_from_slice(b"ee");
        let handle = session.add_torrent(AddTorrent::from_bytes(metadata), Some(AddTorrentOptions {
            output_folder: Some(dir.to_string_lossy().into_owned()),
            ..Default::default()
        })).await.unwrap().into_handle().unwrap();
        let id = handle.id();
        let job = TorrentJob { dir: dir.clone(), torrent: Some((session.clone(), id)), cleaned: false };
        let (started, ready) = tokio::sync::oneshot::channel();
        let staging_cleanup = TorrentStagingCleanup(staging.clone());
        let task = tokio::spawn(async move {
            let _job = job;
            let _staging = staging_cleanup;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), async {
            while session.get(id.into()).is_some() || dir.exists() || staging.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap();
        session.stop().await;
    }

    #[tokio::test]
    async fn quick_request_asks_again_and_stages_the_selected_file() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let root = tempfile::tempdir().unwrap();
        let seed_dir = root.path().join("seed");
        tokio::fs::create_dir(&seed_dir).await.unwrap();
        tokio::fs::write(seed_dir.join("song.flac"), b"abc").await.unwrap();

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let announce = format!("http://{}/announce", listener.local_addr().unwrap());
        let mut metadata = format!(
            "d8:announce{}:{announce}4:infod6:lengthi3e4:name9:song.flac12:piece lengthi16384e6:pieces20:",
            announce.len()
        ).into_bytes();
        // SHA-1 of the complete piece, "abc".
        metadata.extend_from_slice(&[
            0xa9, 0x99, 0x3e, 0x36, 0x47, 0x06, 0x81, 0x6a, 0xba, 0x3e,
            0x25, 0x71, 0x78, 0x50, 0xc2, 0x6c, 0x9c, 0xd0, 0xd8, 0x9d,
        ]);
        metadata.extend_from_slice(b"ee");
        let torrent = librqbit::torrent_from_bytes::<librqbit::ByteBuf>(&metadata).unwrap();
        let locator = format!("magnet:?xt=urn:btih:{}", torrent.info_hash.as_string());
        super::super::inspect::cache_torrent_bytes(&torrent.info_hash.as_string(), &metadata);

        let seed = Session::new_with_opts(seed_dir, SessionOptions {
            disable_dht: true,
            listen_port_range: Some(49152..65535),
            ..Default::default()
        }).await.unwrap();
        let port = seed.tcp_listen_port().unwrap().to_be_bytes();
        // The first announce of both requests finds nobody; only asking again finds the seed.
        let seed_appears = Instant::now() + Duration::from_secs(10);
        let tracker = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0; 4096];
                if stream.read(&mut request).await.is_err() {
                    continue;
                }
                let mut body = b"d8:completei1e10:incompletei0e8:intervali3600e5:peers".to_vec();
                if Instant::now() >= seed_appears {
                    body.extend_from_slice(&[b'6', b':', 127, 0, 0, 1, port[0], port[1]]);
                } else {
                    body.extend_from_slice(b"0:");
                }
                body.push(b'e');
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                if stream.write_all(header.as_bytes()).await.is_ok() {
                    let _ = stream.write_all(&body).await;
                }
            }
        });
        let seeding = seed.add_torrent(AddTorrent::from_bytes(metadata), Some(AddTorrentOptions {
            overwrite: true,
            ..Default::default()
        })).await.unwrap().into_handle().unwrap();
        tokio::time::timeout(Duration::from_secs(5), seeding.wait_until_completed()).await.unwrap().unwrap();

        let requests = [TorrentFileRequest {
            track_index: 7, file_index: 0, file_size: 3,
            track: TrackDownload { title: "Song".into(), ..Default::default() },
        }];
        let counter = Arc::new(AtomicUsize::new(0));
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        let winner = Winner::default();
        let attempt = |lane| {
            transfer_attempt(
                None, "quick-request-test", root.path(), &locator, &requests,
                1, Arc::clone(&counter), Some(sender.clone()), lane, &winner, None,
            )
        };
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            race_lanes(&winner, attempt(Lane::Patient), || attempt(Lane::Quick)),
        ).await;
        tracker.abort();
        seed.stop().await;

        let result = result.unwrap().unwrap();
        assert_eq!(winner.holder(), Some(Lane::Quick));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert!(result.failed_tracks.is_empty());
        let (path, track, index) = receiver.try_recv().unwrap();
        assert_eq!(track.title, "Song");
        assert_eq!(index, 7);
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"abc");
    }

    #[tokio::test]
    async fn http_locators_are_never_handed_to_the_engine() {
        for locator in ["http://127.0.0.1/a.torrent", "https://torrent.invalid/a.torrent"] {
            assert!(torrent_source(locator).await.is_err(), "{locator}");
        }
        let cached = "https://torrent.invalid/cached.torrent";
        super::super::inspect::cache_torrent_bytes(cached, b"torrent metadata");
        assert!(matches!(torrent_source(cached).await, Ok(AddTorrent::TorrentFileBytes(_))));
        let magnet = "magnet:?xt=urn:btih:fedcba9876543210fedcba9876543210fedcba98";
        assert!(matches!(torrent_source(magnet).await, Ok(AddTorrent::Url(_))));
    }

    #[tokio::test]
    async fn each_request_has_its_own_session_and_uses_cached_metadata() {
        let root = tempfile::tempdir().unwrap();
        let mut metadata = b"d4:infod6:lengthi3e4:name12:recovery.mp312:piece lengthi16384e6:pieces20:".to_vec();
        metadata.extend_from_slice(&[1; 20]);
        metadata.extend_from_slice(b"ee");
        let hash = librqbit::torrent_from_bytes::<librqbit::ByteBuf>(&metadata).unwrap().info_hash.as_string();
        let locator = format!("magnet:?xt=urn:btih:{hash}");
        super::super::inspect::cache_torrent_bytes(&hash, &metadata);
        let cache_root = root.path().join("cache");
        let (patient, _) = add_torrent(
            None, "lanes", &cache_root, &root.path().join("a"), &locator, &[0], 1, Lane::Patient,
        ).await.unwrap();
        // Without the cache this magnet has no tracker and no DHT to resolve it in the quick session.
        let (quick, handle) = add_torrent(
            None, "lanes", &cache_root, &root.path().join("b"), &locator, &[0], 1, Lane::Quick,
        ).await.unwrap();
        assert_eq!(handle.info_hash().as_string(), hash);
        assert!(!Arc::ptr_eq(&patient, &quick));
        // The quick session did not replace the DHT session.
        assert!(Arc::ptr_eq(&patient, &get_or_init_session(cache_root, false).await.unwrap()));
        patient.stop().await;
        quick.stop().await;
    }

    #[test]
    fn torrent_cache_root_lives_inside_library_temp_dir() {
        let lib = Path::new("/library/music");
        assert_eq!(torrent_cache_root(lib), Path::new("/library/music/.temp/torrents"));
    }

    #[test]
    fn validate_relative_path_rejects_traversal_and_absolute_paths() {
        assert!(validate_relative_path(Path::new("CD1/01 Track.flac")).is_ok());
        assert!(validate_relative_path(Path::new("../secret.flac")).is_err());
        assert!(validate_relative_path(Path::new("")).is_err());
    }

    #[tokio::test]
    async fn staged_audio_file_survives_job_dir_deletion() {
        let root = std::env::temp_dir().join(format!("blacktape-torrent-stage-test-{}", Uuid::new_v4()));
        let job_dir = root.join(format!("job-{}", Uuid::new_v4()));
        let staging_dir = torrent_staging_dir(&root, "task-123/test");
        tokio::fs::create_dir_all(&job_dir).await.unwrap();
        tokio::fs::create_dir_all(&staging_dir).await.unwrap();

        let src = job_dir.join("05 The End.flac");
        let dst = staging_dir.join("05_05 The End.flac");
        tokio::fs::write(&src, b"flac-bytes").await.unwrap();
        copy_or_move_file(&src, &dst).await.unwrap();

        tokio::fs::remove_dir_all(&job_dir).await.unwrap();
        assert!(!job_dir.exists());
        assert!(dst.exists());
        assert_eq!(tokio::fs::read(&dst).await.unwrap(), b"flac-bytes");

        let _ = tokio::fs::remove_dir_all(&root).await;
    }

    #[test]
    fn swarm_without_peers_stalls_at_the_no_peers_timeout() {
        let start = Instant::now();
        let watch = SwarmWatch::new(start, Lane::Patient);
        assert_eq!(watch.stall(start + TORRENT_NO_PEERS_TIMEOUT - Duration::from_secs(1)), None);
        assert_eq!(watch.stall(start + TORRENT_NO_PEERS_TIMEOUT), Some(Stall::NoPeers));
    }

    #[test]
    fn swarm_with_peers_but_no_bytes_stalls_at_the_silent_peers_timeout() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        watch.observe(SwarmSample { found: 3, connected: 1, ..Default::default() }, start + Duration::from_secs(1));
        // Peers are known, so the longer silent-peers timeout applies instead of the no-peers one.
        assert_eq!(watch.stall(start + TORRENT_NO_PEERS_TIMEOUT), None);
        // A later sample reporting no peers does not forget the ones found earlier.
        watch.observe(SwarmSample::default(), start + TORRENT_NO_PEERS_TIMEOUT);
        assert_eq!(watch.seen.found, 3);
        assert_eq!(watch.stall(start + TORRENT_SILENT_PEERS_TIMEOUT - Duration::from_secs(1)), None);
        assert_eq!(watch.stall(start + TORRENT_SILENT_PEERS_TIMEOUT), Some(Stall::SilentPeers));
    }

    #[test]
    fn unverified_bytes_keep_the_swarm_alive_until_fetching_stops() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        let last_growth = start + Duration::from_secs(30);
        watch.observe(SwarmSample { fetched: 100, found: 2, connected: 1, ..Default::default() }, last_growth);
        // Past the silent-peers timeout, but bytes are still arriving, so nothing is stalled yet.
        assert_eq!(watch.stall(start + TORRENT_SILENT_PEERS_TIMEOUT), None);
        // Fetching stopped at the last growth; the stall fires after TORRENT_STALL_TIMEOUT of silence.
        assert_eq!(watch.stall(last_growth + TORRENT_STALL_TIMEOUT - Duration::from_secs(1)), None);
        assert_eq!(watch.stall(last_growth + TORRENT_STALL_TIMEOUT), Some(Stall::Interrupted));
    }

    #[test]
    fn fetched_bytes_without_verified_pieces_interrupt_after_the_unverified_timeout() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        // Fetched bytes grow every 5 seconds, so the stall timeout never fires; verified never grows.
        for elapsed in (5..TORRENT_UNVERIFIED_TIMEOUT.as_secs()).step_by(5) {
            let now = start + Duration::from_secs(elapsed);
            watch.observe(SwarmSample { fetched: elapsed, found: 1, connected: 1, ..Default::default() }, now);
            assert_eq!(watch.stall(now), None, "at {elapsed}s");
        }
        assert_eq!(watch.stall(start + TORRENT_UNVERIFIED_TIMEOUT - Duration::from_secs(1)), None);
        assert_eq!(watch.stall(start + TORRENT_UNVERIFIED_TIMEOUT), Some(Stall::Interrupted));
    }

    #[test]
    fn waiting_message_reports_peers_before_data_and_data_after() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        watch.observe(SwarmSample { found: 4, connected: 2, ..Default::default() }, start);
        let message = watch.waiting_message(start + Duration::from_secs(3));
        assert!(message.starts_with("Waiting for torrent seeders"), "{message}");
        assert!(message.contains("peers found: 4"), "{message}");
        assert!(message.contains("connected: 2"), "{message}");

        watch.observe(SwarmSample { fetched: 10, found: 4, connected: 2, ..Default::default() }, start + Duration::from_secs(4));
        let message = watch.waiting_message(start + Duration::from_secs(5));
        assert!(message.starts_with("Waiting for torrent data"), "{message}");
        assert!(message.contains("connected peers: 2"), "{message}");
    }

    #[test]
    fn start_failure_turns_quick_start_timeouts_into_no_peers() {
        assert!(matches!(start_failure(Lane::Quick, ADD_TIMED_OUT.into()), TransferError::NoPeers));
        assert!(matches!(start_failure(Lane::Quick, INIT_TIMED_OUT.into()), TransferError::NoPeers));
        assert!(matches!(start_failure(Lane::Quick, "disk full".into()), TransferError::Other(m) if m == "disk full"));
        assert!(matches!(start_failure(Lane::Patient, ADD_TIMED_OUT.into()), TransferError::Other(m) if m == ADD_TIMED_OUT));
        assert!(matches!(start_failure(Lane::Patient, INIT_TIMED_OUT.into()), TransferError::Other(m) if m == INIT_TIMED_OUT));
    }

    #[test]
    fn patient_start_timeout_is_longer_than_the_quick_one() {
        assert!(Lane::Patient.start_timeout() > Lane::Quick.start_timeout());
    }

    #[test]
    fn fetched_bytes_alone_are_not_verified_data() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        watch.observe(SwarmSample { fetched: 100, found: 1, connected: 1, ..Default::default() }, start);
        assert!(watch.has_data());
        assert!(!watch.has_verified_data());
        watch.observe(SwarmSample { verified: 64, fetched: 100, found: 1, connected: 1, ..Default::default() }, start + Duration::from_secs(1));
        assert!(watch.has_verified_data());
    }

    #[tokio::test]
    async fn retry_once_without_peers_attempts_again_after_no_peers() {
        use std::cell::Cell;

        let calls = Cell::new(0);
        let result = retry_once_without_peers(|| {
            calls.set(calls.get() + 1);
            let call = calls.get();
            async move { if call == 1 { Err(TransferError::NoPeers) } else { Ok(call) } }
        })
        .await;
        assert!(matches!(result, Ok(2)));
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn retry_once_without_peers_gives_up_after_two_no_peers() {
        use std::cell::Cell;

        let calls = Cell::new(0);
        let result: Result<(), TransferError> = retry_once_without_peers(|| {
            calls.set(calls.get() + 1);
            async { Err(TransferError::NoPeers) }
        })
        .await;
        assert!(matches!(result, Err(TransferError::NoPeers)));
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn retry_once_without_peers_returns_first_success_or_other_error_directly() {
        use std::cell::Cell;

        let calls = Cell::new(0);
        let ok: Result<u8, TransferError> = retry_once_without_peers(|| {
            calls.set(calls.get() + 1);
            async { Ok(7) }
        })
        .await;
        assert!(matches!(ok, Ok(7)));
        assert_eq!(calls.get(), 1);

        calls.set(0);
        let other: Result<u8, TransferError> = retry_once_without_peers(|| {
            calls.set(calls.get() + 1);
            async { Err(TransferError::Other("boom".into())) }
        })
        .await;
        assert!(matches!(other, Err(TransferError::Other(m)) if m == "boom"));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn unconnected_found_peers_stall_fast_instead_of_waiting_full_silent_timeout() {
        let start = Instant::now();
        let mut watch = SwarmWatch::new(start, Lane::Patient);
        watch.observe(SwarmSample { found: 4, connected: 0, ..Default::default() }, start);
        assert_eq!(watch.stall(start + TORRENT_UNCONNECTED_PEERS_TIMEOUT - Duration::from_secs(1)), None);
        assert_eq!(watch.stall(start + TORRENT_UNCONNECTED_PEERS_TIMEOUT), Some(Stall::SilentPeers));
    }

    #[test]
    fn swarm_race_winner_grants_first_verified_candidate_only() {
        let race = SwarmRaceWinner::default();
        assert_eq!(race.holder(), None);
        assert!(race.keeps(0, false));
        assert!(race.keeps(1, true));
        assert_eq!(race.holder(), Some(1));
        assert!(!race.keeps(0, false));
        assert!(!race.keeps(0, true));
        assert!(race.keeps(1, true));
    }

    #[tokio::test]
    async fn race_candidate_torrents_wakes_alternate_early_when_primary_fails() {
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            race_candidate_torrents(2, |idx| async move {
                if idx == 0 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Err(NO_TORRENT_PEERS.into())
                } else {
                    Ok(TorrentTransferResult {
                        completed: vec![(PathBuf::from("track.flac"), TrackDownload::default(), 0)],
                        failed_tracks: Vec::new(),
                        failure: None,
                    })
                }
            }),
        )
        .await
        .expect("alternate should wake immediately without waiting full head-start")
        .unwrap();
        assert_eq!(result.completed.len(), 1);
    }
}
