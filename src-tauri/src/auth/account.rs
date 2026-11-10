//! Connected platform accounts (Settings → Accounts): status check, sign-in and sign-out.
//!
//! YouTube keeps its Google session in the login window's browser profile, and yt-dlp reads the cookies from
//! there. So "connected" means that profile is signed in, and the check asks YouTube itself, in a hidden window
//! with the same profile, instead of trusting a file on disk.

use super::youtube::{self, BROWSER_USER_AGENT, LOGIN_WINDOW};
use crate::AuthLock;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;
use url::Url;

/// Sent with the provider after a sign-in or sign-out, so every open window can refresh its status.
pub const ACCOUNT_CHANGED_EVENT: &str = "account-changed";

const CHECK_WINDOW: &str = "youtube-account-check";
const CLEAR_WINDOW: &str = "youtube-account-clear";
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const PING_URL: &str = "https://www.youtube.com/generate_204";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountProvider {
    Youtube,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountState {
    Connected,
    Disconnected,
    /// The state could not be determined; `message` says why.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountStatus {
    pub provider: AccountProvider,
    pub state: AccountState,
    /// Human-readable detail for the settings page.
    pub message: Option<String>,
}

impl AccountStatus {
    fn new(provider: AccountProvider, state: AccountState, message: Option<&str>) -> Self {
        Self {
            provider,
            state,
            message: message.map(str::to_string),
        }
    }
}

pub fn notify_changed(app: &AppHandle, provider: AccountProvider) {
    let _ = app.emit(ACCOUNT_CHANGED_EVENT, provider);
}

/// What the check page reported about the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageReport {
    LoggedIn,
    LoggedOut,
    Unreadable,
}

/// What a finished page load means for the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostVerdict {
    /// A YouTube page: read its login flag.
    ReadPage,
    /// The cookie-consent page: YouTube shows it only to visitors without a session.
    SignedOut,
    /// A Google sign-in page: a redirect step for a signed-in profile, or the end of the road for a signed-out one.
    /// Decided only if nothing else follows (see `check_youtube`).
    SignIn,
    /// Any other intermediate page; wait for the next one.
    Ignore,
}

fn classify_host(host: Option<&str>) -> HostVerdict {
    match host {
        Some("www.youtube.com" | "youtube.com" | "m.youtube.com") => HostVerdict::ReadPage,
        Some("consent.youtube.com" | "consent.google.com") => HostVerdict::SignedOut,
        Some("accounts.google.com") => HostVerdict::SignIn,
        _ => HostVerdict::Ignore,
    }
}

/// Reads `state=in|out|unknown` from the check script's `tauri://account-check?…` navigation.
fn report_from_query(query: &str) -> PageReport {
    let state = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("state="))
        .unwrap_or("");
    match state {
        "in" => PageReport::LoggedIn,
        "out" => PageReport::LoggedOut,
        _ => PageReport::Unreadable,
    }
}

fn status_from_report(provider: AccountProvider, report: PageReport) -> AccountStatus {
    match report {
        PageReport::LoggedIn => AccountStatus::new(provider, AccountState::Connected, None),
        PageReport::LoggedOut => AccountStatus::new(
            provider,
            AccountState::Disconnected,
            Some("Not signed in, or the session has expired."),
        ),
        PageReport::Unreadable => AccountStatus::new(
            provider,
            AccountState::Error,
            Some("YouTube opened, but its page could not be read. Try again in a moment."),
        ),
    }
}

/// Reads YouTube's own login flag. ytcfg can appear a moment after the load event, so it retries a few times.
const CHECK_SCRIPT: &str = r#"
(function () {
  function report(state) { window.location.href = "tauri://account-check?state=" + state; }
  function attempt(n) {
    var loggedIn;
    try {
      if (window.ytcfg && typeof window.ytcfg.get === "function") loggedIn = window.ytcfg.get("LOGGED_IN");
    } catch (e) {}
    if (loggedIn === undefined && window.yt && window.yt.config_) loggedIn = window.yt.config_.LOGGED_IN;
    if (loggedIn === true) return report("in");
    if (loggedIn === false) return report("out");
    if (n < 3) return setTimeout(function () { attempt(n + 1); }, 1000);
    report("unknown");
  }
  attempt(0);
})();
"#;

fn ping_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default()
    })
}

/// Tells "no internet / YouTube blocked" apart from "not signed in" before opening a browser window.
async fn youtube_reachable() -> Result<(), String> {
    match ping_client().get(PING_URL).send().await {
        Ok(_) => Ok(()),
        Err(e) if e.is_timeout() => Err("YouTube did not respond (timed out).".into()),
        Err(e) if e.is_connect() => {
            Err("Cannot connect to YouTube. Check your internet connection.".into())
        }
        Err(e) => Err(format!("Cannot reach YouTube: {e}")),
    }
}

type ReportSender = Arc<Mutex<Option<oneshot::Sender<PageReport>>>>;

fn send_report(tx: &ReportSender, report: PageReport) {
    if let Ok(mut guard) = tx.lock() {
        if let Some(sender) = guard.take() {
            let _ = sender.send(report);
        }
    }
}

async fn check_youtube(app: &AppHandle) -> AccountStatus {
    let provider = AccountProvider::Youtube;
    let profile = match youtube::profile_dir(app) {
        Ok(dir) => dir,
        Err(e) => return AccountStatus::new(provider, AccountState::Error, Some(&e)),
    };
    if !profile.exists() {
        return AccountStatus::new(provider, AccountState::Disconnected, Some("Not signed in."));
    }
    if let Err(e) = youtube_reachable().await {
        return AccountStatus::new(provider, AccountState::Error, Some(&e));
    }
    if let Some(stale) = app.get_webview_window(CHECK_WINDOW) {
        let _ = stale.destroy();
    }

    let url = match Url::parse("https://www.youtube.com/") {
        Ok(url) => url,
        Err(e) => return AccountStatus::new(provider, AccountState::Error, Some(&e.to_string())),
    };
    let (tx, rx) = oneshot::channel::<PageReport>();
    let tx: ReportSender = Arc::new(Mutex::new(Some(tx)));
    let tx_nav = Arc::clone(&tx);
    let tx_load = Arc::clone(&tx);
    let stuck_on_sign_in = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stuck_load = Arc::clone(&stuck_on_sign_in);

    let built = WebviewWindowBuilder::new(app, CHECK_WINDOW, WebviewUrl::External(url))
        .data_directory(profile)
        .user_agent(BROWSER_USER_AGENT)
        .title("Blacktape | Checking YouTube account")
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .on_navigation(move |url| {
            if url.scheme() == "tauri" && url.host_str() == Some("account-check") {
                send_report(&tx_nav, report_from_query(url.query().unwrap_or("")));
                return false;
            }
            true
        })
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let verdict = classify_host(payload.url().host_str());
            stuck_load.store(
                verdict == HostVerdict::SignIn,
                std::sync::atomic::Ordering::SeqCst,
            );
            match verdict {
                HostVerdict::ReadPage => {
                    let _ = window.eval(CHECK_SCRIPT);
                }
                HostVerdict::SignedOut => send_report(&tx_load, PageReport::LoggedOut),
                HostVerdict::SignIn | HostVerdict::Ignore => {}
            }
        })
        .build();

    let window = match built {
        Ok(window) => window,
        Err(e) => {
            return AccountStatus::new(
                provider,
                AccountState::Error,
                Some(&format!("Could not open the check window: {e}")),
            )
        }
    };

    let result = tokio::time::timeout(CHECK_TIMEOUT, rx).await;
    let _ = window.destroy();

    match result {
        Ok(Ok(report)) => status_from_report(provider, report),
        // Google kept the profile on its sign-in page: the session is gone.
        _ if stuck_on_sign_in.load(std::sync::atomic::Ordering::SeqCst) => {
            status_from_report(provider, PageReport::LoggedOut)
        }
        _ => AccountStatus::new(
            provider,
            AccountState::Error,
            Some("YouTube did not finish loading in 20 seconds. Check your connection and try again."),
        ),
    }
}

/// Removes the Google session from the login profile (only in this app; the Google account itself stays signed in
/// elsewhere).
async fn clear_youtube_profile(app: &AppHandle) -> Result<(), String> {
    let profile = youtube::profile_dir(app)?;
    if profile.exists() {
        if let Some(stale) = app.get_webview_window(CLEAR_WINDOW) {
            let _ = stale.destroy();
        }
        let url = Url::parse("about:blank").map_err(|e| e.to_string())?;
        let window = WebviewWindowBuilder::new(app, CLEAR_WINDOW, WebviewUrl::External(url))
            .data_directory(profile)
            .user_agent(BROWSER_USER_AGENT)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .build()
            .map_err(|e| format!("Could not open the browser profile: {e}"))?;
        let cleared = window
            .clear_all_browsing_data()
            .map_err(|e| format!("Could not clear the browser profile: {e}"));
        // The engine clears in the background; give it a moment before the window goes away.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let _ = window.destroy();
        cleared?;
    }
    match tokio::fs::remove_file(youtube::cookie_file(app)?).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Could not delete the saved cookies: {e}")),
    }
}

fn busy_status(provider: AccountProvider) -> AccountStatus {
    AccountStatus::new(
        provider,
        AccountState::Error,
        Some("A YouTube sign-in is in progress (maybe started by a download). Finish it, then check again."),
    )
}

#[tauri::command]
pub async fn account_status(
    app: AppHandle,
    auth_lock: State<'_, AuthLock>,
    provider: AccountProvider,
) -> Result<AccountStatus, String> {
    // A download may be signing in right now; the check must not wait for that window.
    let Ok(_guard) = auth_lock.0.try_lock() else {
        return Ok(busy_status(provider));
    };
    if app.get_webview_window(LOGIN_WINDOW).is_some() {
        return Ok(busy_status(provider));
    }
    Ok(match provider {
        AccountProvider::Youtube => check_youtube(&app).await,
    })
}

#[tauri::command]
pub async fn account_connect(
    app: AppHandle,
    auth_lock: State<'_, AuthLock>,
    provider: AccountProvider,
) -> Result<AccountStatus, String> {
    match provider {
        AccountProvider::Youtube => {
            if let Some(window) = app.get_webview_window(LOGIN_WINDOW) {
                let _ = window.show();
                let _ = window.set_focus();
                return Ok(busy_status(provider));
            }
            // Opens YouTube hidden: a signed-in profile finishes on its own, otherwise Google's login is shown.
            if youtube::launch_youtube_login(app.clone(), false, auth_lock.clone())
                .await
                .is_err()
            {
                return Ok(AccountStatus::new(
                    provider,
                    AccountState::Disconnected,
                    Some("Sign-in was cancelled."),
                ));
            }
            let _guard = auth_lock.0.lock().await;
            Ok(check_youtube(&app).await)
        }
    }
}

#[tauri::command]
pub async fn account_disconnect(
    app: AppHandle,
    auth_lock: State<'_, AuthLock>,
    provider: AccountProvider,
) -> Result<AccountStatus, String> {
    match provider {
        AccountProvider::Youtube => {
            // Closing an open sign-in window ends that sign-in and releases the lock.
            if let Some(window) = app.get_webview_window(LOGIN_WINDOW) {
                let _ = window.destroy();
            }
            let _guard = auth_lock.0.lock().await;
            clear_youtube_profile(&app).await?;
            notify_changed(&app, provider);

            let after = check_youtube(&app).await;
            Ok(match after.state {
                AccountState::Connected => AccountStatus::new(
                    provider,
                    AccountState::Error,
                    Some("Sign-out did not take effect: YouTube still sees the session. Try again."),
                ),
                AccountState::Disconnected => {
                    AccountStatus::new(provider, AccountState::Disconnected, Some("Signed out."))
                }
                // The session is gone from disk even if YouTube could not be asked.
                AccountState::Error => AccountStatus::new(
                    provider,
                    AccountState::Disconnected,
                    Some("Signed out (YouTube could not be reached to confirm)."),
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_host_reads_youtube_pages() {
        assert_eq!(classify_host(Some("www.youtube.com")), HostVerdict::ReadPage);
        assert_eq!(classify_host(Some("youtube.com")), HostVerdict::ReadPage);
        assert_eq!(classify_host(Some("m.youtube.com")), HostVerdict::ReadPage);
    }

    #[test]
    fn classify_host_treats_consent_as_signed_out_and_sign_in_as_pending() {
        assert_eq!(classify_host(Some("accounts.google.com")), HostVerdict::SignIn);
        assert_eq!(classify_host(Some("consent.youtube.com")), HostVerdict::SignedOut);
        assert_eq!(classify_host(Some("consent.google.com")), HostVerdict::SignedOut);
    }

    #[test]
    fn classify_host_ignores_everything_else() {
        assert_eq!(classify_host(None), HostVerdict::Ignore);
        assert_eq!(classify_host(Some("music.youtube.com")), HostVerdict::Ignore);
        assert_eq!(classify_host(Some("accounts.youtube.com")), HostVerdict::Ignore);
        assert_eq!(classify_host(Some("evil.com")), HostVerdict::Ignore);
        assert_eq!(classify_host(Some("youtube.com.evil.com")), HostVerdict::Ignore);
    }

    #[test]
    fn report_from_query_reads_login_flag() {
        assert_eq!(report_from_query("state=in"), PageReport::LoggedIn);
        assert_eq!(report_from_query("state=out"), PageReport::LoggedOut);
        assert_eq!(report_from_query("x=1&state=in"), PageReport::LoggedIn);
        assert_eq!(report_from_query("state=out&x=1"), PageReport::LoggedOut);
    }

    #[test]
    fn report_from_query_rejects_unknown_or_malformed_state() {
        assert_eq!(report_from_query("state=unknown"), PageReport::Unreadable);
        assert_eq!(report_from_query(""), PageReport::Unreadable);
        assert_eq!(report_from_query("state="), PageReport::Unreadable);
        assert_eq!(report_from_query("foo=bar"), PageReport::Unreadable);
        assert_eq!(report_from_query("state=IN"), PageReport::Unreadable);
    }

    #[test]
    fn status_from_logged_in_report_is_connected_without_message() {
        let status = status_from_report(AccountProvider::Youtube, PageReport::LoggedIn);
        assert_eq!(status.provider, AccountProvider::Youtube);
        assert_eq!(status.state, AccountState::Connected);
        assert_eq!(status.message, None);
    }

    #[test]
    fn status_from_logged_out_report_is_disconnected_with_message() {
        let status = status_from_report(AccountProvider::Youtube, PageReport::LoggedOut);
        assert_eq!(status.provider, AccountProvider::Youtube);
        assert_eq!(status.state, AccountState::Disconnected);
        assert!(status.message.is_some());
    }

    #[test]
    fn status_from_unreadable_report_is_error_with_message() {
        let status = status_from_report(AccountProvider::Youtube, PageReport::Unreadable);
        assert_eq!(status.provider, AccountProvider::Youtube);
        assert_eq!(status.state, AccountState::Error);
        assert!(status.message.is_some());
    }

    #[test]
    fn account_provider_deserializes_only_lowercase_youtube() {
        let provider: AccountProvider = serde_json::from_str("\"youtube\"").unwrap();
        assert_eq!(provider, AccountProvider::Youtube);
        assert!(serde_json::from_str::<AccountProvider>("\"YouTube\"").is_err());
        assert!(serde_json::from_str::<AccountProvider>("\"bandcamp\"").is_err());
    }

    #[test]
    fn connected_status_serializes_to_expected_json() {
        let status = AccountStatus {
            provider: AccountProvider::Youtube,
            state: AccountState::Connected,
            message: None,
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"provider": "youtube", "state": "connected", "message": null})
        );
    }

    #[test]
    fn error_status_serializes_state_as_error() {
        let status = AccountStatus {
            provider: AccountProvider::Youtube,
            state: AccountState::Error,
            message: Some("boom".to_string()),
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["state"], "error");
        assert_eq!(value["message"], "boom");
    }

    #[test]
    fn busy_status_is_error_with_message() {
        let status = busy_status(AccountProvider::Youtube);
        assert_eq!(status.provider, AccountProvider::Youtube);
        assert_eq!(status.state, AccountState::Error);
        assert!(status.message.is_some());
    }
}
