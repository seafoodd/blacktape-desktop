use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use sentry::{protocol::Event, Breadcrumb, ClientInitGuard, ClientOptions, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub const TELEMETRY_SETTING_KEY: &str = "telemetry_enabled";
pub const ONBOARDING_COMPLETED_KEY: &str = "onboarding_completed";

static TELEMETRY_ENABLED: AtomicBool = AtomicBool::new(true);
static INSTALL_ID: OnceLock<String> = OnceLock::new();

pub fn set_enabled(enabled: bool) {
    TELEMETRY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    TELEMETRY_ENABLED.load(Ordering::Relaxed)
}

/// Loads or creates a random UUIDv4 in `config_dir/install_id` (no hardware or IP fingerprinting).
pub fn init_install_id(config_dir: &Path) {
    let id_path = config_dir.join("install_id");
    let id = std::fs::read_to_string(&id_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let new_id = uuid::Uuid::new_v4().to_string();
            let _ = std::fs::create_dir_all(config_dir);
            let _ = std::fs::write(&id_path, &new_id);
            new_id
        });
    let _ = INSTALL_ID.set(id);
}

fn scrub_home_dir(text: &str) -> String {
    let mut cleaned = text.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if !home.is_empty() {
                cleaned = cleaned.replace(&home, "<HOME>");
                // Also catch escaped Windows backslashes in JSON/debug strings
                let escaped = home.replace('\\', "\\\\");
                cleaned = cleaned.replace(&escaped, "<HOME>");
            }
        }
    }
    cleaned
}

fn scrub_value(val: &mut sentry::protocol::Value) {
    match val {
        sentry::protocol::Value::String(s) => *s = scrub_home_dir(s),
        sentry::protocol::Value::Array(arr) => {
            for v in arr {
                scrub_value(v);
            }
        }
        sentry::protocol::Value::Object(map) => {
            for v in map.values_mut() {
                scrub_value(v);
            }
        }
        _ => {}
    }
}

fn sanitize_event(mut event: Event<'static>) -> Option<Event<'static>> {
    if !is_enabled() {
        return None;
    }

    // Strip machine hostname and any accidental user identifiers
    event.server_name = None;
    let anon_id = INSTALL_ID.get().cloned();
    event.user = Some(sentry::User {
        id: anon_id,
        ..Default::default()
    });

    if let Some(ref mut msg) = event.message {
        *msg = scrub_home_dir(msg);
    }

    for exc in &mut event.exception.values {
        if let Some(ref mut val) = exc.value {
            *val = scrub_home_dir(val);
        }
    }

    for crumb in &mut event.breadcrumbs.values {
        if let Some(ref mut msg) = crumb.message {
            *msg = scrub_home_dir(msg);
        }
        for val in crumb.data.values_mut() {
            scrub_value(val);
        }
    }

    for val in event.extra.values_mut() {
        scrub_value(val);
    }

    Some(event)
}

fn sanitize_breadcrumb(mut crumb: Breadcrumb) -> Option<Breadcrumb> {
    if !is_enabled() {
        return None;
    }
    if let Some(ref mut msg) = crumb.message {
        *msg = scrub_home_dir(msg);
    }
    for val in crumb.data.values_mut() {
        scrub_value(val);
    }
    Some(crumb)
}

/// Initializes the global tracing subscriber with console output and the Sentry tracing layer.
/// Safe to call multiple times (subsequent calls in tests or restarts are no-ops).
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(sentry::integrations::tracing::BreadcrumbLayer::default())
        .try_init();
}

pub fn init() -> Option<ClientInitGuard> {
    // Only active in release/CI builds where SENTRY_DSN is injected at compile time
    let dsn = option_env!("SENTRY_DSN")?;
    if dsn.trim().is_empty() {
        return None;
    }

    let mut options = ClientOptions::default();
    options.release = sentry::release_name!();
    options.send_default_pii = false;
    options.before_send = Some(Arc::new(sanitize_event));
    options.before_breadcrumb = Some(Arc::new(sanitize_breadcrumb));

    let guard = sentry::init((dsn, options));

    Some(guard)
}

/// Classifies a raw download or yt-dlp error string into a normalized category without private URLs or paths.
pub fn classify_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if lower.contains("did not get any data blocks") {
        "no_data_blocks"
    } else if lower.contains("403") || lower.contains("forbidden") {
        "http_403_forbidden"
    } else if lower.contains("429") || lower.contains("too many requests") {
        "rate_limited_429"
    } else if lower.contains("not a bot") || lower.contains("login required") || lower.contains("sign in to confirm") {
        "bot_or_login_required"
    } else if lower.contains("private video") || lower.contains("members-only") || lower.contains("granted access") {
        "restricted_access"
    } else if lower.contains("age-restricted") || lower.contains("age restricted") {
        "age_restricted"
    } else if lower.contains("no output file matching") {
        "no_output_file"
    } else if lower.contains("part-frag")
        || lower.contains("fragment not found")
        || lower.contains("unable to download video")
    {
        "fragment_download_failed"
    } else if lower.contains("errno 2") || lower.contains("no such file or directory") {
        "file_not_found"
    } else if lower.contains("permission denied") || lower.contains("access is denied") {
        "permission_denied"
    } else if lower.contains("no video formats found")
        || lower.contains("requested format is not available")
        || lower.contains("produced no audio file")
    {
        "format_not_available"
    } else if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("unreachable")
        || lower.contains("connection")
        || lower.contains("temporary failure in name resolution")
        || lower.contains("network request failed")
    {
        "network_timeout"
    } else if lower.contains("auth_window_closed") || lower.contains("auth window closed") {
        "auth_window_closed"
    } else if lower.contains("rustypipe failed")
        || lower.contains("failed to fetch youtube")
        || lower.contains("failed to fetch album")
        || lower.contains("video details")
        || lower.contains("music details")
    {
        "metadata_fetch_failed"
    } else if lower.contains("failed to parse") || lower.contains("parse error") {
        "parser_failed"
    } else if lower.contains("404") || lower.contains("not found") {
        "not_found_404"
    } else {
        "unknown_download_error"
    }
}

/// Classifies a raw download failure and sends a privacy-safe telemetry event to Sentry with detailed error context.
pub fn report_download_error(platform: &str, stage: &str, raw_error: &str) {
    report_download_error_with_context(platform, stage, raw_error, None, None, None);
}

/// Reports a download failure with full context: target URL, track name, and full stderr/stdout.
pub fn report_download_error_with_context(
    platform: &str,
    stage: &str,
    raw_error: &str,
    target_url: Option<&str>,
    track_name: Option<&str>,
    detailed_log: Option<&str>,
) {
    if !is_enabled() {
        return;
    }

    let error_kind = classify_error(raw_error);
    let sanitized_error = scrub_home_dir(raw_error);

    // Extract the most informative line for the title
    let summary_line = sanitized_error
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("ERROR:") || l.starts_with("Failed") || l.contains("Error"))
        .or_else(|| sanitized_error.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or(error_kind);

    sentry::with_scope(
        |scope| {
            scope.set_tag("platform", platform);
            scope.set_tag("stage", stage);
            scope.set_tag("error_kind", error_kind);

            if let Some(url) = target_url {
                if let Ok(parsed) = url::Url::parse(url) {
                    if let Some(host) = parsed.host_str() {
                        scope.set_tag("url_host", host);
                    }
                }
                scope.set_extra("target_url", serde_json::Value::String(scrub_home_dir(url)));
            }

            if let Some(name) = track_name {
                scope.set_extra("track_name", serde_json::Value::String(scrub_home_dir(name)));
            }

            let full_details = match detailed_log {
                Some(log) => scrub_home_dir(log),
                None => sanitized_error.clone(),
            };

            scope.set_extra(
                "error_details",
                serde_json::Value::String(full_details),
            );

            if error_kind != "unknown_download_error" {
                // Group known issues by (platform, stage, error_kind)
                scope.set_fingerprint(Some(&[platform, stage, error_kind]));
            } else {
                // Distinct unknown errors get distinct issues in Sentry so they can be identified
                scope.set_fingerprint(Some(&[platform, stage, summary_line]));
            }
        },
        || {
            sentry::capture_message(
                &format!("[{platform}] {stage} failed ({error_kind}): {summary_line}"),
                Level::Error,
            );
        },
    );
}

/// Classifies a raw parser failure and sends a privacy-safe telemetry event to Sentry with detailed error context.
pub fn report_parser_failure(platform: &str, stage: &str, raw_error: &str) {
    report_download_error_with_context(platform, stage, raw_error, None, None, None);
}

/// Reports a non-fatal platform/parser failure without leaking track names or URLs.
pub fn report_parser_error(platform: &str, stage: &str, error_kind: &str) {
    report_download_error_with_context(platform, stage, error_kind, None, None, None);
}

/// Reports an error with normalized tags, fingerprinting, and sanitized details in Sentry's Additional Data.
pub fn report_error_with_details(platform: &str, stage: &str, raw_error: &str) {
    report_download_error_with_context(platform, stage, raw_error, None, None, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::{Exception, Values};
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_classify_error() {
        assert_eq!(classify_error("ERROR: Did not get any data blocks"), "no_data_blocks");
        assert_eq!(classify_error("HTTP Error 403: Forbidden"), "http_403_forbidden");
        assert_eq!(classify_error("HTTP Error 429: Too Many Requests"), "rate_limited_429");
        assert_eq!(classify_error("Sign in to confirm you're not a bot"), "bot_or_login_required");
        assert_eq!(classify_error("Private video. Sign in if you've been granted access"), "restricted_access");
        assert_eq!(classify_error("This video is age-restricted"), "age_restricted");
        assert_eq!(classify_error("Process succeeded, but no output file matching '1.*' was found in dir"), "no_output_file");
        assert_eq!(
            classify_error("ERROR: Unable to download video: [Errno 2] No such file or directory: '<HOME>\\blacktape-lib\\.temp\\batch_1795381982395\\1.mp4.part-Frag56'"),
            "fragment_download_failed"
        );
        assert_eq!(
            classify_error("[download] fragment not found; Skipping fragment 78 ..."),
            "fragment_download_failed"
        );
        assert_eq!(
            classify_error("[Errno 2] No such file or directory: 'some/file.mp3'"),
            "file_not_found"
        );
        assert_eq!(
            classify_error("Permission denied: 'C:\\test\\locked.mp3'"),
            "permission_denied"
        );
        assert_eq!(classify_error("no video formats found"), "format_not_available");
        assert_eq!(classify_error("Connection timed out after 30000ms"), "network_timeout");
        assert_eq!(classify_error("auth_window_closed"), "auth_window_closed");
        assert_eq!(classify_error("Failed to fetch YouTube video details: not found"), "metadata_fetch_failed");
        assert_eq!(classify_error("RustyPipe failed to fetch album: 404"), "metadata_fetch_failed");
        assert_eq!(classify_error("Failed to parse JSON payload"), "parser_failed");
        assert_eq!(classify_error("Item not found"), "not_found_404");
        assert_eq!(classify_error("some totally unmapped failure occurred"), "unknown_download_error");
    }

    #[test]
    fn test_toggle_enabled() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
    }

    #[test]
    fn test_scrub_home_dir() {
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            let input = format!("Error at {home}\\test\\path");
            let scrubbed = scrub_home_dir(&input);
            assert!(!scrubbed.contains(&home));
            assert!(scrubbed.contains("<HOME>"));

            let escaped_home = home.replace('\\', "\\\\");
            let input_escaped = format!("Error at {escaped_home}\\\\test\\\\path");
            let scrubbed_escaped = scrub_home_dir(&input_escaped);
            assert!(!scrubbed_escaped.contains(&escaped_home));
            assert!(scrubbed_escaped.contains("<HOME>"));
        }
    }

    #[test]
    fn test_sanitize_breadcrumb() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(false);
        let crumb = Breadcrumb {
            message: Some("test message".to_string()),
            ..Default::default()
        };
        assert!(sanitize_breadcrumb(crumb).is_none());

        set_enabled(true);
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            let crumb = Breadcrumb {
                message: Some(format!("Error at {home}/something")),
                ..Default::default()
            };
            let res = sanitize_breadcrumb(crumb).expect("breadcrumb should be retained");
            assert!(res.message.unwrap().contains("<HOME>"));

            let mut data_map = std::collections::BTreeMap::new();
            data_map.insert("path".to_string(), sentry::protocol::Value::String(format!("{home}/music/song.mp3")));
            data_map.insert(
                "nested".to_string(),
                sentry::protocol::Value::Array(vec![sentry::protocol::Value::String(format!("{home}/cache"))]),
            );
            let crumb_with_data = Breadcrumb {
                data: data_map,
                ..Default::default()
            };
            let res_data = sanitize_breadcrumb(crumb_with_data).expect("breadcrumb should be retained");
            let path_val = res_data.data.get("path").unwrap().as_str().unwrap();
            assert!(path_val.contains("<HOME>"));
            assert!(!path_val.contains(&home));
            let nested_val = res_data.data.get("nested").unwrap().as_array().unwrap();
            assert!(nested_val[0].as_str().unwrap().contains("<HOME>"));
            assert!(!nested_val[0].as_str().unwrap().contains(&home));
        }
    }

    #[test]
    fn test_sanitize_event() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(false);
        let event = Event::default();
        assert!(sanitize_event(event).is_none());

        set_enabled(true);
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| "C:\\Users\\dummy".to_string());

        let mut event = Event::default();
        event.server_name = Some("secret-workstation".into());
        event.message = Some(format!("Crash in {home}/app"));
        event.exception = Values {
            values: vec![Exception {
                value: Some(format!("File not found: {home}/data")),
                ..Default::default()
            }],
        };
        event.breadcrumbs = Values {
            values: vec![Breadcrumb {
                message: Some(format!("Action at {home}/lib")),
                ..Default::default()
            }],
        };

        let mut extra_map = std::collections::BTreeMap::new();
        extra_map.insert("debug_dir".to_string(), sentry::protocol::Value::String(format!("{home}/temp")));
        event.extra = extra_map;

        let sanitized = sanitize_event(event).expect("event should be kept");
        assert!(sanitized.server_name.is_none());
        assert!(!sanitized.message.as_ref().unwrap().contains(&home));
        assert!(sanitized.message.as_ref().unwrap().contains("<HOME>"));
        assert!(!sanitized.exception.values[0].value.as_ref().unwrap().contains(&home));
        assert!(sanitized.exception.values[0].value.as_ref().unwrap().contains("<HOME>"));
        assert!(!sanitized.breadcrumbs.values[0].message.as_ref().unwrap().contains(&home));
        assert!(sanitized.breadcrumbs.values[0].message.as_ref().unwrap().contains("<HOME>"));
        let extra_dir = sanitized.extra.get("debug_dir").unwrap().as_str().unwrap();
        assert!(extra_dir.contains("<HOME>"));
        assert!(!extra_dir.contains(&home));
    }

    #[test]
    fn test_telemetry_enabled_atomic_toggle() {
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
        assert!(is_enabled());
    }

    #[test]
    fn test_init_tracing_is_idempotent() {
        // Safe to call multiple times without panicking
        init_tracing();
        init_tracing();
        tracing::info!("Tracing subscriber test message");
    }

    #[test]
    fn test_init_install_id_file_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_install_id(temp_dir.path());
        let id_file = temp_dir.path().join("install_id");
        assert!(id_file.exists());
        let content = std::fs::read_to_string(&id_file).unwrap();
        assert!(!content.trim().is_empty());
        assert!(uuid::Uuid::parse_str(content.trim()).is_ok());
    }

    #[test]
    #[ignore = "requires network and live SENTRY_DSN; run with: cargo test --manifest-path src-tauri/Cargo.toml test_live_sentry_ping -- --ignored --nocapture"]
    fn test_live_sentry_ping() {
        let guard = init().expect("SENTRY_DSN must be set at compile time");
        sentry::capture_message("Blacktape telemetry test ping", Level::Info);
        drop(guard); // flushes queue to Sentry
        println!("Sentry test event dispatched successfully.");
    }
}
