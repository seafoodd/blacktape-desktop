use super::{inspect, release::InspectedRelease};
use base64::Engine;
use librqbit::{torrent_from_bytes_ext, ByteBuf};
use serde::{de::DeserializeOwned, Deserialize};
use std::time::Duration;
use tauri::AppHandle;
#[cfg(not(test))]
use std::sync::Mutex;
#[cfg(not(test))]
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;

const KEYRING_SERVICE: &str = "blacktape-desktop";
const KEYRING_USER: &str = "rutracker";
#[cfg(not(test))]
const ORIGIN: &str = "https://rutracker.net";
const OFFICIAL_ORIGINS: [&str; 2] = ["https://rutracker.net", "https://rutracker.org"];
#[cfg(not(test))]
const WINDOW: &str = "rutracker-session";
#[cfg(not(test))]
const LOOKUP: &str = include_str!("rutracker.js");
const LIMIT: usize = 4 * 1024 * 1024;
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long a request may wait behind the others for its turn in the browser.
#[cfg(not(test))]
const QUEUE_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(not(test))]
const CHALLENGE_WAIT: Duration = Duration::from_secs(6);
#[cfg(not(test))]
const LOGIN_SUBMIT_WAIT: Duration = Duration::from_secs(12);
#[cfg(not(test))]
static BROWSER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, serde::Serialize, Deserialize, PartialEq, Eq)]
struct SavedCredentials {
    username: String,
    password: String,
}

#[cfg(not(test))]
fn save_credentials(username: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|_| "Could not access the system credential store.".to_owned())?;
    let payload = serde_json::to_string(&SavedCredentials {
        username: username.to_owned(),
        password: password.to_owned(),
    })
    .map_err(|_| "Could not encode RuTracker credentials.".to_owned())?;
    entry
        .set_password(&payload)
        .map_err(|_| "Could not save RuTracker credentials to the system credential store.".to_owned())
}

fn load_credentials() -> Option<SavedCredentials> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    let raw = entry.get_password().ok()?;
    serde_json::from_str::<SavedCredentials>(&raw).ok().filter(|creds| {
        !creds.username.trim().is_empty() && !creds.password.is_empty()
    })
}

pub(super) fn saved_username() -> Option<String> {
    load_credentials().map(|creds| creds.username)
}

pub(super) fn has_saved_credentials() -> bool {
    load_credentials().is_some()
}

#[cfg(not(test))]
fn clear_credentials() {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        let _ = entry.delete_credential();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AuthFailure {
    InvalidCredentials,
    CaptchaRequired,
    CloudflareChallenge,
    FormChanged,
    Timeout,
    Unavailable,
}

impl AuthFailure {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::InvalidCredentials => "invalid_credentials",
            Self::CaptchaRequired => "captcha_required",
            Self::CloudflareChallenge => "cloudflare_challenge",
            Self::FormChanged => "form_changed",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
        }
    }

    pub(super) fn message(self) -> &'static str {
        match self {
            Self::InvalidCredentials => "Invalid RuTracker username or password.",
            Self::CaptchaRequired => "RuTracker asked for a CAPTCHA. Use browser sign-in to complete it.",
            Self::CloudflareChallenge => "RuTracker stopped at a Cloudflare browser check. Use browser sign-in to complete it.",
            Self::FormChanged => "Could not find the RuTracker login form. Use browser sign-in instead.",
            Self::Timeout => "RuTracker sign-in timed out. Try again or use browser sign-in.",
            Self::Unavailable => "Could not reach RuTracker.",
        }
    }

    pub(super) fn should_offer_browser(self) -> bool {
        matches!(
            self,
            Self::CaptchaRequired | Self::CloudflareChallenge | Self::FormChanged | Self::Timeout
        )
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuTrackerSignInResult {
    pub signed_in: bool,
    pub username: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<&'static str>,
    pub offer_browser_fallback: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Failure {
    SignIn,
    CloudflareChallenge,
    Blocked,
    Timeout,
    RateLimited,
    Malformed,
    ResponseTooLarge,
    Unavailable,
}

impl Failure {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::SignIn => "Sign in to RuTracker from Settings, then try again.",
            Self::CloudflareChallenge => "RuTracker stopped this request at a Cloudflare browser check. Open Sign in to check whether the page lets you continue, then retry Test.",
            Self::Blocked => "RuTracker declined the request. Check access in the RuTracker sign-in window.",
            Self::Timeout => "RuTracker request timed out.",
            Self::RateLimited => "RuTracker is rate limiting requests.",
            Self::Malformed => "RuTracker returned an unexpected response.",
            Self::ResponseTooLarge => "RuTracker response exceeded the size limit.",
            Self::Unavailable => "Could not reach RuTracker.",
        }
    }
}

fn official_origin(url: &Url) -> bool {
    url.scheme() == "https" && matches!(url.host_str(), Some("rutracker.org" | "rutracker.net"))
        && url.port().is_none() && url.username().is_empty() && url.password().is_none()
}

#[cfg(not(test))]
fn browser(app: &AppHandle) -> Result<WebviewWindow, Failure> {
    if let Some(window) = app.get_webview_window(WINDOW) {
        return Ok(window);
    }
    let profile = app.path().app_data_dir().map_err(|_| Failure::Unavailable)?.join("rutracker-profile");
    let url = Url::parse(&format!("{ORIGIN}/forum/index.php")).map_err(|_| Failure::Unavailable)?;
    let builder = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::External(url))
        .data_directory(profile)
        .visible(false)
        .focused(false)
        .title("Blacktape | RuTracker")
        .inner_size(940.0, 700.0)
        .on_navigation(official_origin)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_download(|_, _| false);
    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(*b"BlacktapeRuTrack");
    let window = builder.build().map_err(|_| Failure::Unavailable)?;
    let session = window.clone();
    // Keep session-only cookies alive after the user closes the visible sign-in window.
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = session.hide();
        }
    });
    Ok(window)
}

#[cfg(not(test))]
fn spawn_login_watcher(app: AppHandle, window: WebviewWindow) {
    tauri::async_runtime::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(600)).await;
            if !window.is_visible().unwrap_or(false) {
                break;
            }
            let Ok(_guard) = BROWSER.try_lock() else {
                continue;
            };
            let Ok(Some(username)) = evaluate::<Option<String>>(&window, logged_in_username_script()).await else {
                continue;
            };
            let username = username.trim().to_owned();
            if username.is_empty() {
                continue;
            }
            let _ = window.hide();
            super::commands::record_rutracker_login(&app, &username).await;
            let _ = app.emit("rutracker-status-changed", ());
            let _ = app.emit("torrent-indexers-changed", ());
            break;
        }
    });
}

#[cfg(not(test))]
pub(super) async fn open_login(app: &AppHandle) -> Result<(), String> {
    let window = {
        let _guard = BROWSER.lock().await;
        let window = browser(app).map_err(|error| error.message())?;
        let url = Url::parse(&format!("{ORIGIN}/forum/login.php")).map_err(|_| Failure::Unavailable.message())?;
        window.navigate(url).map_err(|_| Failure::Unavailable.message())?;
        window.show().map_err(|_| Failure::Unavailable.message())?;
        window.set_focus().map_err(|_| Failure::Unavailable.message())?;
        window
    };
    spawn_login_watcher(app.clone(), window);
    Ok(())
}

#[cfg(not(test))]
async fn evaluate<T: DeserializeOwned>(window: &WebviewWindow, script: String) -> Result<T, Failure> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = Mutex::new(Some(send));
    window.eval_with_callback(script, move |value| {
        if let Some(send) = send.lock().ok().and_then(|mut guard| guard.take()) {
            let _ = send.send(value);
        }
    }).map_err(|_| Failure::Unavailable)?;
    let value = receive.await.map_err(|_| Failure::Unavailable)?;
    serde_json::from_str(&value).map_err(|_| Failure::Malformed)
}

#[cfg(not(test))]
#[derive(Deserialize)]
#[serde(untagged)]
enum BrowserResult<T> {
    Failed { error: Failure },
    Data { data: T },
}

/// Runs `task` once `lock` is free. Waiting for the turn does not use up the time of the task itself.
async fn queued<T>(
    lock: &tokio::sync::Mutex<()>,
    queue_timeout: Duration,
    run_timeout: Duration,
    task: impl std::future::Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    let _turn = tokio::time::timeout(queue_timeout, lock.lock()).await.map_err(|_| Failure::Timeout)?;
    tokio::time::timeout(run_timeout, task).await.map_err(|_| Failure::Timeout)?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PageState {
    Loading,
    Challenge,
    Ready,
}

fn page_state_script(origins: &[&str]) -> String {
    let origin_check = origins
        .iter()
        .map(|origin| format!("location.origin === '{origin}'"))
        .collect::<Vec<_>>()
        .join(" || ");
    format!(
        "(() => {{
            if (!({origin_check}) || document.readyState === 'loading') return 'loading';
            if (document.querySelector('#page_container, #main_content, #logged-in-username, form[action*=\"login.php\"]')) return 'ready';
            if (document.querySelector('#challenge-running, #challenge-form, .cf-browser-verification, [name=\"cf-turnstile-response\"]') || /just a moment/i.test(document.title || '')) return 'challenge';
            return 'ready';
        }})()"
    )
}

#[cfg(not(test))]
async fn wait_until_ready(window: &WebviewWindow) -> Result<(), Failure> {
    let script = page_state_script(&OFFICIAL_ORIGINS);
    let challenge_deadline = tokio::time::Instant::now() + CHALLENGE_WAIT;
    loop {
        match evaluate::<PageState>(window, script.clone()).await? {
            PageState::Ready => return Ok(()),
            PageState::Challenge if tokio::time::Instant::now() >= challenge_deadline => {
                return Err(Failure::CloudflareChallenge);
            }
            PageState::Loading | PageState::Challenge => {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
        }
    }
}

#[cfg(not(test))]
async fn request<T: DeserializeOwned>(app: &AppHandle, path: String, kind: &str) -> Result<T, Failure> {
    queued(&BROWSER, QUEUE_TIMEOUT, TIMEOUT, async {
        let window = browser(app)?;
        wait_until_ready(&window).await?;
        let request = format!("__blacktape_{}", uuid::Uuid::new_v4().simple());
        let path = serde_json::to_string(&path).map_err(|_| Failure::Malformed)?;
        window.eval(format!("(() => {{
            const state = {{ controller: new AbortController() }};
            window.{request} = state;
            void ({LOOKUP})({path}, '{kind}', state.controller.signal).then(
                value => state.result = value,
                () => state.result = {{ error: 'unavailable' }},
            );
        }})();")).map_err(|_| Failure::Unavailable)?;
        let cleanup = BrowserRequest { window, key: request };
        loop {
            let script = format!("window.{}?.result ?? null", cleanup.key);
            match evaluate(&cleanup.window, script).await? {
                Some(BrowserResult::Data { data }) => return Ok(data),
                Some(BrowserResult::Failed { error }) => return Err(error),
                None => tokio::time::sleep(Duration::from_millis(120)).await,
            }
        }
    }).await
}

#[cfg(not(test))]
fn logged_in_username_script() -> String {
    "(() => document.querySelector('#logged-in-username')?.textContent?.trim() || null)()".into()
}

fn login_submit_script(username: &str, password: &str) -> Result<String, AuthFailure> {
    let user_json = serde_json::to_string(username).map_err(|_| AuthFailure::Unavailable)?;
    let pass_json = serde_json::to_string(password).map_err(|_| AuthFailure::Unavailable)?;
    Ok(format!(
        "(() => {{
            const loggedIn = document.querySelector('#logged-in-username')?.textContent?.trim();
            if (loggedIn) return {{ status: 'signed_in', username: loggedIn }};
            if (document.querySelector('img[src*=\"captcha\"], input[name=\"cap_code\"], [name=\"cf-turnstile-response\"]')) {{
                return {{ status: 'captcha_required' }};
            }}
            const form = document.querySelector('form[action*=\"login.php\"]');
            const userInput = form?.querySelector('input[name=\"login_username\"]');
            const passInput = form?.querySelector('input[name=\"login_password\"]');
            if (!form || !userInput || !passInput) return {{ status: 'form_changed' }};
            userInput.value = {user_json};
            passInput.value = {pass_json};
            userInput.dispatchEvent(new Event('input', {{ bubbles: true }}));
            passInput.dispatchEvent(new Event('input', {{ bubbles: true }}));
            const submit = form.querySelector('input[name=\"login\"], input[type=\"submit\"], button[type=\"submit\"]');
            if (submit) submit.click();
            else form.submit();
            return {{ status: 'submitted' }};
        }})()"
    ))
}

fn login_outcome_script() -> String {
    "(() => {
        const loggedIn = document.querySelector('#logged-in-username')?.textContent?.trim();
        if (loggedIn) return { status: 'signed_in', username: loggedIn };
        if (document.querySelector('img[src*=\"captcha\"], input[name=\"cap_code\"], [name=\"cf-turnstile-response\"]')) {
            return { status: 'captcha_required' };
        }
        const form = document.querySelector('form[action*=\"login.php\"]');
        if (form && (document.querySelector('.mrg_16, .warnColor1, #error-msg') || location.pathname.includes('login.php'))) {
            return { status: 'invalid_credentials' };
        }
        return { status: 'pending' };
    })()".into()
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum LoginDomState {
    SignedIn { username: String },
    Submitted,
    CaptchaRequired,
    FormChanged,
    InvalidCredentials,
    Pending,
}

#[cfg(not(test))]
fn map_wait_failure(error: Failure) -> AuthFailure {
    match error {
        Failure::CloudflareChallenge => AuthFailure::CloudflareChallenge,
        Failure::Timeout => AuthFailure::Timeout,
        _ => AuthFailure::Unavailable,
    }
}

#[cfg(not(test))]
async fn perform_browser_login(app: &AppHandle, username: &str, password: &str) -> Result<String, AuthFailure> {
    let script = login_submit_script(username, password)?;
    let _guard = tokio::time::timeout(QUEUE_TIMEOUT, BROWSER.lock())
        .await
        .map_err(|_| AuthFailure::Timeout)?;
    let window = browser(app).map_err(|_| AuthFailure::Unavailable)?;
    let url = Url::parse(&format!("{ORIGIN}/forum/login.php")).map_err(|_| AuthFailure::Unavailable)?;
    window.navigate(url).map_err(|_| AuthFailure::Unavailable)?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    wait_until_ready(&window).await.map_err(map_wait_failure)?;

    match evaluate::<LoginDomState>(&window, script)
        .await
        .map_err(|_| AuthFailure::Unavailable)?
    {
        LoginDomState::SignedIn { username } => return Ok(username),
        LoginDomState::CaptchaRequired => return Err(AuthFailure::CaptchaRequired),
        LoginDomState::FormChanged => return Err(AuthFailure::FormChanged),
        LoginDomState::Submitted => {}
        LoginDomState::InvalidCredentials => return Err(AuthFailure::InvalidCredentials),
        LoginDomState::Pending => {}
    }

    tokio::time::sleep(Duration::from_millis(400)).await;
    let deadline = tokio::time::Instant::now() + LOGIN_SUBMIT_WAIT;
    loop {
        wait_until_ready(&window).await.map_err(map_wait_failure)?;
        match evaluate::<LoginDomState>(&window, login_outcome_script())
            .await
            .map_err(|_| AuthFailure::Unavailable)?
        {
            LoginDomState::SignedIn { username } => return Ok(username),
            LoginDomState::CaptchaRequired => return Err(AuthFailure::CaptchaRequired),
            LoginDomState::InvalidCredentials => return Err(AuthFailure::InvalidCredentials),
            LoginDomState::FormChanged => return Err(AuthFailure::FormChanged),
            LoginDomState::Pending | LoginDomState::Submitted => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(AuthFailure::Timeout);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

#[cfg(not(test))]
pub(super) async fn sign_in_with_credentials(
    app: &AppHandle,
    username: &str,
    password: &str,
) -> Result<String, AuthFailure> {
    let trimmed_user = username.trim();
    if trimmed_user.is_empty() || password.is_empty() {
        return Err(AuthFailure::InvalidCredentials);
    }
    let logged_in_user = perform_browser_login(app, trimmed_user, password).await?;
    if let Err(err) = save_credentials(trimmed_user, password) {
        tracing::warn!("RuTracker login succeeded but credential save failed: {err}");
    }
    Ok(logged_in_user)
}

#[cfg(not(test))]
pub(super) async fn sign_out(app: &AppHandle) {
    clear_credentials();
    let _guard = BROWSER.lock().await;
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
        let _ = window.eval("document.querySelector('#logged-in-username')?.remove()");
        let _ = window.clear_all_browsing_data();
        if let Ok(url) = Url::parse(&format!("{ORIGIN}/forum/login.php")) {
            let _ = window.navigate(url);
        }
    } else if let Ok(data_dir) = app.path().app_data_dir() {
        let profile = data_dir.join("rutracker-profile");
        if profile.exists() {
            let _ = std::fs::remove_dir_all(profile);
        }
    }
}

#[cfg(not(test))]
pub(super) async fn live_session_username(app: &AppHandle) -> Option<String> {
    let window = app.get_webview_window(WINDOW)?;
    let _guard = BROWSER.try_lock().ok()?;
    evaluate::<Option<String>>(&window, logged_in_username_script())
        .await
        .ok()
        .flatten()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

// Desktop webviews need the Windows application manifest, which the unit-test runner lacks.
#[cfg(test)]
async fn request<T: DeserializeOwned>(_app: &AppHandle, _path: String, _kind: &str) -> Result<T, Failure> {
    Err(Failure::Unavailable)
}

#[cfg(test)]
pub(super) async fn open_login(_app: &AppHandle) -> Result<(), String> {
    Err(Failure::Unavailable.message().into())
}

#[cfg(test)]
pub(super) async fn sign_in_with_credentials(
    _app: &AppHandle,
    _username: &str,
    _password: &str,
) -> Result<String, AuthFailure> {
    Err(AuthFailure::Unavailable)
}

#[cfg(test)]
pub(super) async fn sign_out(_app: &AppHandle) {}

#[cfg(test)]
pub(super) async fn live_session_username(_app: &AppHandle) -> Option<String> {
    None
}

#[cfg(not(test))]
struct BrowserRequest {
    window: WebviewWindow,
    key: String,
}

#[cfg(not(test))]
impl Drop for BrowserRequest {
    fn drop(&mut self) {
        let key = &self.key;
        let _ = self.window.eval(format!("window.{key}?.controller.abort(); delete window.{key};"));
    }
}

#[derive(Deserialize)]
pub(super) struct Row {
    pub(super) title: String,
    pub(super) link: String,
    pub(super) seeders: Option<u64>,
}

fn search_path(query: &str) -> String {
    let query = query.split(|ch: char| !ch.is_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>().join("%");
    let mut url = Url::parse("https://rutracker.org/forum/tracker.php").expect("static RuTracker URL");
    url.query_pairs_mut().append_pair("nm", &query);
    format!("{}?{}", url.path(), url.query().unwrap_or_default())
}

#[cfg(not(test))]
async fn try_silent_relogin(app: &AppHandle) -> Result<(), Failure> {
    let creds = load_credentials().ok_or(Failure::SignIn)?;
    match perform_browser_login(app, &creds.username, &creds.password).await {
        Ok(username) => {
            super::commands::record_rutracker_login(app, &username).await;
            Ok(())
        }
        Err(AuthFailure::InvalidCredentials) => {
            clear_credentials();
            Err(Failure::SignIn)
        }
        Err(AuthFailure::CaptchaRequired | AuthFailure::CloudflareChallenge) => {
            Err(Failure::CloudflareChallenge)
        }
        Err(_) => Err(Failure::SignIn),
    }
}

#[cfg(test)]
async fn try_silent_relogin(_app: &AppHandle) -> Result<(), Failure> {
    Err(Failure::SignIn)
}

pub(super) async fn search(app: &AppHandle, query: &str) -> Result<Vec<Row>, Failure> {
    let path = search_path(query);
    match request(app, path.clone(), "search").await {
        Err(Failure::SignIn) => {
            try_silent_relogin(app).await?;
            request(app, path, "search").await
        }
        other => other,
    }
}

pub(super) fn topic_from_link(link: &str) -> Option<u64> {
    let base = Url::parse("https://rutracker.org/forum/").ok()?;
    let url = base.join(link).ok()?;
    if !official_origin(&url) || url.path() != "/forum/dl.php" || url.fragment().is_some() { return None; }
    let mut pairs = url.query_pairs();
    let (key, value) = pairs.next()?;
    if key != "t" || pairs.next().is_some() { return None; }
    value.parse().ok().filter(|id| *id != 0)
}

pub(super) fn topic(input: &str) -> Option<u64> {
    let digits = input.strip_prefix("rutracker:")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    digits.parse().ok().filter(|id| *id != 0)
}

fn public_tracker(input: &[u8]) -> Option<String> {
    let url = Url::parse(std::str::from_utf8(input).ok()?).ok()?;
    let host = url.host_str()?;
    if !matches!(host, "bt.t-ru.org" | "bt2.t-ru.org" | "bt3.t-ru.org" | "bt4.t-ru.org")
        || !matches!(url.scheme(), "http" | "https") || url.port().is_some()
        || url.path() != "/ann" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    // RuTracker publishes a separate public announce URL for magnets. Never give the engine an account key.
    Some(format!("{}://{host}/ann?magnet", url.scheme()))
}

fn prepare_torrent(bytes: &[u8], expected: Option<&str>) -> Result<(InspectedRelease, Vec<u8>), String> {
    let release = inspect::parse_metainfo(bytes, expected)?;
    let metainfo = torrent_from_bytes_ext::<ByteBuf>(bytes).map_err(|_| "Invalid RuTracker torrent metadata.")?;
    if metainfo.meta.info.private {
        return Err("This RuTracker release requires private tracker support.".into());
    }
    let tracker = metainfo.meta.iter_announce().find_map(|tracker| public_tracker(tracker.as_ref()))
        .ok_or("This release has no supported RuTracker announce endpoint.")?;
    let mut safe = format!("d8:announce{}:{tracker}4:info", tracker.len()).into_bytes();
    safe.extend_from_slice(metainfo.info_bytes.as_ref());
    safe.push(b'e');
    Ok((release, safe))
}

pub(super) async fn torrent(app: &AppHandle, topic: u64, expected: Option<&str>) -> Result<(InspectedRelease, Vec<u8>), String> {
    let path = format!("/forum/dl.php?t={topic}");
    let encoded: String = match request(app, path.clone(), "torrent").await {
        Err(Failure::SignIn) => {
            try_silent_relogin(app).await.map_err(Failure::message)?;
            request(app, path, "torrent").await.map_err(Failure::message)?
        }
        other => other.map_err(Failure::message)?,
    };
    if encoded.len() > LIMIT.div_ceil(3) * 4 { return Err(Failure::ResponseTooLarge.message().into()); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| "Invalid RuTracker torrent response.")?;
    prepare_torrent(&bytes, expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tracker: &str, private: bool) -> Vec<u8> {
        format!("d8:announce{}:{tracker}4:infod6:lengthi1e4:name10:Track.flac12:piece lengthi16384e6:pieces20:abcdefghijklmnopqrst{}ee",
            tracker.len(), if private { "7:privatei1e" } else { "" }).into_bytes()
    }

    #[test]
    fn tracker_replies_can_only_identify_a_topic_on_the_official_origin() {
        for link in [
            "dl.php?t=42",
            "/forum/dl.php?t=42",
            "https://rutracker.org/forum/dl.php?t=42",
            "https://rutracker.net/forum/dl.php?t=42",
        ] {
            assert_eq!(topic_from_link(link), Some(42));
        }
        for link in ["//evil.test/forum/dl.php?t=42", "https://rutracker.org.evil.test/forum/dl.php?t=42",
            "https://rutracker.net.evil.test/forum/dl.php?t=42",
            "http://rutracker.org/forum/dl.php?t=42", "https://rutracker.org:81/forum/dl.php?t=42",
            "dl.php?t=42&token=hidden", "dl.php?t=0", "viewtopic.php?t=42"] {
            assert_eq!(topic_from_link(link), None, "{link}");
        }
        assert_eq!(topic("rutracker:42"), Some(42));
        assert!(topic("rutracker:42?anything").is_none());
        assert_eq!(search_path("Björk – Debut & live"), "/forum/tracker.php?nm=Bj%C3%B6rk%25Debut%25live");
    }

    #[test]
    fn public_download_preserves_the_verified_hash_and_keeps_account_keys_out_of_the_engine() {
        let original = fixture("http://bt2.t-ru.org/ann?uk=fixture-key", false);
        let (release, safe) = prepare_torrent(&original, None).unwrap();
        assert_eq!(release.tracks[0].codec, "FLAC");
        assert!(!String::from_utf8_lossy(&safe).contains("fixture-key"));
        let parsed = torrent_from_bytes_ext::<ByteBuf>(&safe).unwrap();
        assert_eq!(parsed.meta.info_hash.as_string(), release.info_hash);
        assert_eq!(parsed.meta.announce.unwrap().as_ref(), b"http://bt2.t-ru.org/ann?magnet");
        assert!(prepare_torrent(&original, Some("wrong hash")).is_err());
        assert!(prepare_torrent(&fixture("http://bt2.t-ru.org/ann?uk=fixture-key", true), None).is_err());
        assert!(prepare_torrent(&fixture("https://bt.t-ru.org.evil.test/ann", false), None).is_err());
    }

    #[tokio::test]
    async fn waiting_for_the_turn_does_not_count_against_the_run_timeout() {
        let lock = tokio::sync::Mutex::new(());
        let held = lock.lock().await;
        let release = async {
            tokio::time::sleep(Duration::from_millis(600)).await;
            drop(held);
        };
        let (result, ()) = tokio::join!(
            queued(&lock, Duration::from_secs(10), Duration::from_millis(200), async { Ok::<u8, Failure>(7) }),
            release,
        );
        assert_eq!(result.unwrap(), 7);
    }

    #[tokio::test]
    async fn queued_task_times_out_when_the_lock_is_not_freed_in_time_and_never_runs() {
        let lock = tokio::sync::Mutex::new(());
        let _held = lock.lock().await;
        let ran = std::sync::atomic::AtomicBool::new(false);
        let result = queued(&lock, Duration::from_millis(100), Duration::from_secs(5), async {
            ran.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok::<u8, Failure>(1)
        }).await;
        assert!(matches!(result, Err(Failure::Timeout)));
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn task_slower_than_the_run_timeout_gives_timeout() {
        let lock = tokio::sync::Mutex::new(());
        let result = queued(&lock, Duration::from_secs(10), Duration::from_millis(200), async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<u8, Failure>(1)
        }).await;
        assert!(matches!(result, Err(Failure::Timeout)));
    }

    #[test]
    fn page_state_script_checks_origin_forum_dom_and_cloudflare_challenge() {
        let script = page_state_script(&OFFICIAL_ORIGINS);
        assert!(script.contains("https://rutracker.net"));
        assert!(script.contains("https://rutracker.org"));
        assert!(script.contains("#page_container"));
        assert!(script.contains("just a moment"));
        assert_eq!(serde_json::from_str::<PageState>("\"loading\"").unwrap(), PageState::Loading);
        assert_eq!(serde_json::from_str::<PageState>("\"challenge\"").unwrap(), PageState::Challenge);
        assert_eq!(serde_json::from_str::<PageState>("\"ready\"").unwrap(), PageState::Ready);
    }

    #[test]
    fn login_submit_script_escapes_credentials_and_checks_captcha_and_form() {
        let script = login_submit_script("user\"name", "p@ss'\\word").unwrap();
        assert!(script.contains("\"user\\\"name\""));
        assert!(script.contains("\"p@ss'\\\\word\""));
        assert!(script.contains("login_username"));
        assert!(script.contains("login_password"));
        assert!(script.contains("captcha_required"));
        assert!(script.contains("form_changed"));
        let outcome = login_outcome_script();
        assert!(outcome.contains("invalid_credentials"));
        assert_eq!(
            serde_json::from_str::<LoginDomState>(r#"{"status":"signed_in","username":"alice"}"#).unwrap(),
            LoginDomState::SignedIn { username: "alice".into() }
        );
    }

    #[test]
    fn auth_failure_browser_fallback_flags_match_expected_cases() {
        assert!(!AuthFailure::InvalidCredentials.should_offer_browser());
        assert!(!AuthFailure::Unavailable.should_offer_browser());
        assert!(AuthFailure::CaptchaRequired.should_offer_browser());
        assert!(AuthFailure::CloudflareChallenge.should_offer_browser());
        assert!(AuthFailure::FormChanged.should_offer_browser());
        assert!(AuthFailure::Timeout.should_offer_browser());
        assert_eq!(AuthFailure::CaptchaRequired.code(), "captcha_required");
    }
}
