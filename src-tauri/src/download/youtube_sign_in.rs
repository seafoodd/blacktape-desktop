use crate::auth::{account, youtube};
use crate::AuthLock;
use serde::Deserialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventId, Listener, Manager, WebviewWindow};

const REQUEST: &str = "download-youtube-sign-in-required";
const RESPONSE: &str = "download-youtube-sign-in-response";
const CLOSED: &str = "download-youtube-sign-in-dismissed";
const PROMPT_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const CANCELLED: &str = "YouTube sign-in was cancelled.";
const SKIPPED_SUFFIX: &str = "not in the torrent and not downloaded. Sign in to YouTube in Settings to get such tracks from there.";

/// Why tracks of a torrent download were left out instead of asking the user to sign in.
pub(super) fn skipped_message(count: usize) -> String {
    format!("{count} track(s) {SKIPPED_SUFFIX}")
}

pub(super) fn is_skipped_message(message: &str) -> bool {
    message.ends_with(SKIPPED_SUFFIX)
}

/// Whether the download was meant to come from a torrent: the user picked one, or some track has one.
pub(super) fn is_torrent_download(format: Option<&str>, tracks: &[super::TrackDownload]) -> bool {
    format.is_some_and(|format| format.starts_with("source:torrent"))
        || tracks.iter().any(|track| track.torrent_target.is_some())
}

fn is_youtube(url: &str) -> bool {
    super::ytdlp::download_site(url) == super::ytdlp::DownloadSite::Youtube
}

/// Removes the YouTube addresses of `tracks` and drops the tracks that have no other one. Returns how many
/// were dropped.
fn remove_youtube_sources(tracks: &mut Vec<super::TrackDownload>) -> usize {
    let before = tracks.len();
    tracks.retain_mut(|track| {
        let mut others = std::iter::once(std::mem::take(&mut track.url))
            .chain(std::mem::take(&mut track.fallback_urls))
            .filter(|url| !url.is_empty() && !is_youtube(url));
        let Some(first) = others.next() else { return false };
        track.fallback_urls = others.collect();
        track.url = first;
        true
    });
    before - tracks.len()
}

/// A torrent download never asks to sign in to YouTube. Without a signed-in session its YouTube addresses are
/// left out; returns how many tracks that leaves without a source.
pub(super) async fn leave_out_youtube_unless_signed_in(
    tracks: &mut Vec<super::TrackDownload>,
    signed_in: impl std::future::Future<Output = bool>,
) -> usize {
    let uses_youtube = tracks.iter()
        .any(|track| std::iter::once(&track.url).chain(&track.fallback_urls).any(|url| is_youtube(url)));
    if !uses_youtube || signed_in.await {
        return 0;
    }
    remove_youtube_sources(tracks)
}

/// The saved session only: nothing is shown to the user.
pub(super) async fn has_session(app: &AppHandle) -> bool {
    account::youtube_download_session_ready(app).await.unwrap_or(false)
}

#[derive(Deserialize)]
struct Decision {
    request_id: uuid::Uuid,
    proceed: bool,
}

struct SignInResponse {
    id: uuid::Uuid,
    sender: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
}

impl SignInResponse {
    fn receive(&self, payload: &str) {
        let Ok(decision) = serde_json::from_str::<Decision>(payload) else {
            return;
        };
        if decision.request_id != self.id {
            return;
        }
        if let Some(sender) = self.sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(decision.proceed);
        }
    }
}

struct PendingPrompt {
    window: WebviewWindow,
    listener: EventId,
    id: uuid::Uuid,
}

impl Drop for PendingPrompt {
    fn drop(&mut self) {
        self.window.unlisten(self.listener);
        let _ = self.window.emit_to("main", CLOSED, self.id);
    }
}

async fn ask_to_sign_in(app: &AppHandle) -> Result<bool, String> {
    let window = app.get_webview_window("main").ok_or("Could not show the YouTube sign-in prompt.")?;
    let id = uuid::Uuid::new_v4();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let response = SignInResponse { id, sender: Mutex::new(Some(sender)) };
    let listener = window.listen(RESPONSE, move |event| response.receive(event.payload()));
    let pending = PendingPrompt { window, listener, id };
    pending.window.emit_to("main", REQUEST, id).map_err(|_| "Could not show the YouTube sign-in prompt.")?;
    tokio::time::timeout(PROMPT_TIMEOUT, receiver).await
        .map_err(|_| "Timed out waiting for the YouTube sign-in prompt.".to_string())?
        .map_err(|_| "The YouTube sign-in prompt was closed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prompt_accepts_only_the_first_valid_reply_for_the_current_request() {
        for proceed in [false, true] {
            let id = uuid::Uuid::new_v4();
            let (sender, mut receiver) = tokio::sync::oneshot::channel();
            let response = SignInResponse { id, sender: Mutex::new(Some(sender)) };
            response.receive("invalid");
            response.receive(&serde_json::json!({"request_id": uuid::Uuid::new_v4(), "proceed": !proceed}).to_string());
            assert!(matches!(receiver.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
            response.receive(&serde_json::json!({"request_id": id, "proceed": proceed}).to_string());
            response.receive(&serde_json::json!({"request_id": id, "proceed": !proceed}).to_string());
            assert_eq!(receiver.await.unwrap(), proceed);
        }
    }

    const YT: &str = "https://youtube.com/watch?v=a";
    const YT_MUSIC: &str = "https://music.youtube.com/watch?v=b";
    const BC: &str = "https://artist.bandcamp.com/track/c";

    fn torrent_target() -> crate::download::types::TorrentTarget {
        crate::download::types::TorrentTarget {
            info_hash: "a".repeat(40),
            magnet_uri: "magnet:?xt=urn:btih:aaaa".into(),
            file_index: 0,
            file_path: "a.flac".into(),
            file_size: 1,
            quality_tier: crate::metadata::types::AudioQualityTier::Lossless,
            alternatives: Vec::new(),
        }
    }

    fn sources(tracks: &[crate::download::types::TrackDownload]) -> Vec<(String, Vec<String>)> {
        tracks.iter().map(|track| (track.url.clone(), track.fallback_urls.clone())).collect()
    }

    #[test]
    fn torrent_format_is_a_torrent_download_even_without_tracks() {
        assert!(is_torrent_download(Some("source:torrent:lossless"), &[]));
    }

    #[test]
    fn a_track_with_a_torrent_target_makes_the_download_a_torrent_download() {
        let track = crate::download::types::TrackDownload {
            torrent_target: Some(torrent_target()),
            ..Default::default()
        };
        assert!(is_torrent_download(None, &[Default::default(), track]));
    }

    #[test]
    fn other_formats_without_torrent_targets_are_not_torrent_downloads() {
        let track = crate::download::types::TrackDownload { url: YT.into(), ..Default::default() };
        for format in [Some("best"), None, Some("source:youtube:high")] {
            assert!(!is_torrent_download(format, std::slice::from_ref(&track)));
        }
    }

    #[tokio::test]
    async fn not_signed_in_drops_youtube_only_tracks_and_counts_them() {
        let mut tracks = vec![
            crate::download::types::TrackDownload { url: YT.into(), ..Default::default() },
            crate::download::types::TrackDownload { url: BC.into(), ..Default::default() },
        ];
        let dropped = leave_out_youtube_unless_signed_in(&mut tracks, async { false }).await;
        assert_eq!(dropped, 1);
        assert_eq!(sources(&tracks), vec![(BC.to_string(), Vec::new())]);
    }

    #[tokio::test]
    async fn not_signed_in_keeps_a_bandcamp_fallback_as_the_url() {
        let mut tracks = vec![crate::download::types::TrackDownload {
            url: YT.into(),
            fallback_urls: vec![BC.into()],
            ..Default::default()
        }];
        let dropped = leave_out_youtube_unless_signed_in(&mut tracks, async { false }).await;
        assert_eq!(dropped, 0);
        assert_eq!(sources(&tracks), vec![(BC.to_string(), Vec::new())]);
    }

    #[tokio::test]
    async fn not_signed_in_removes_only_the_youtube_fallbacks_of_a_bandcamp_track() {
        let mut tracks = vec![crate::download::types::TrackDownload {
            url: BC.into(),
            fallback_urls: vec![YT.into(), YT_MUSIC.into()],
            ..Default::default()
        }];
        let dropped = leave_out_youtube_unless_signed_in(&mut tracks, async { false }).await;
        assert_eq!(dropped, 0);
        assert_eq!(sources(&tracks), vec![(BC.to_string(), Vec::new())]);
    }

    #[tokio::test]
    async fn signed_in_leaves_youtube_tracks_unchanged() {
        let mut tracks = vec![
            crate::download::types::TrackDownload { url: YT.into(), ..Default::default() },
            crate::download::types::TrackDownload {
                url: BC.into(),
                fallback_urls: vec![YT_MUSIC.into()],
                ..Default::default()
            },
        ];
        let before = sources(&tracks);
        let dropped = leave_out_youtube_unless_signed_in(&mut tracks, async { true }).await;
        assert_eq!(dropped, 0);
        assert_eq!(sources(&tracks), before);
    }

    #[tokio::test]
    async fn without_youtube_addresses_the_session_is_never_checked() {
        let mut tracks = vec![crate::download::types::TrackDownload { url: BC.into(), ..Default::default() }];
        let dropped = leave_out_youtube_unless_signed_in(&mut tracks, async { panic!("session must not be checked") }).await;
        assert_eq!(dropped, 0);
        assert_eq!(sources(&tracks), vec![(BC.to_string(), Vec::new())]);
    }

    #[test]
    fn skipped_message_is_recognised_and_names_the_count() {
        let message = skipped_message(2);
        assert!(is_skipped_message(&message));
        assert!(message.contains("2 track(s)"));
        assert!(!is_skipped_message(CANCELLED));
    }
}

pub(super) async fn ensure_session(app: &AppHandle, task_id: &str) -> Result<(), String> {
    if account::youtube_download_session_ready(app).await? {
        return Ok(());
    }

    let message = "Waiting for YouTube sign-in…";
    if let Some(registry) = app.try_state::<super::TaskRegistry>() {
        if let Some((current, total)) = registry.set_task_message(task_id, message).await {
            let _ = app.emit("download-task-progress", super::ProgressEvent {
                task_id: task_id.into(), current, total, track_title: message.into(),
            });
        }
    }
    if !ask_to_sign_in(app).await? {
        return Err(CANCELLED.into());
    }

    youtube::launch_youtube_login(app.clone(), false, app.state::<AuthLock>()).await
        .map_err(|error| {
            if error == "Authentication canceled by user." { CANCELLED.into() }
            else { format!("Could not open YouTube sign-in: {error}") }
        })
}
