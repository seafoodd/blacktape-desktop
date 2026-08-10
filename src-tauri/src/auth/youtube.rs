use tauri::{command, AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::fs::{read_to_string, File};
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;

#[command]
pub async fn cookies_are_ready(
    app: AppHandle,
    window: WebviewWindow,
    payload: String,
) -> Result<(), String> {
    println!("[blacktape::auth] IPC Cookie payload received!");

    if let Err(e) = write_netscape_cookies(&app, &payload).await {
        eprintln!("[blacktape::auth ERROR] Failed to write cookie file: {}", e);
        return Err(e);
    }

    println!("[blacktape::auth] Cookies successfully stored. Closing auth window.");
    let _ = window.close();
    Ok(())
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[command]
pub async fn launch_youtube_login(app: AppHandle, force_visible: bool) -> Result<(), String> {
    println!("[blacktape::auth] launch_youtube_login command invoked.");

    if let Some(existing_win) = app.get_webview_window("youtube-login") {
        let _ = existing_win.close();
    }

    let (tx, rx) = oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    // Track whether the user actually authenticated successfully
    let is_authenticated = Arc::new(AtomicBool::new(false));

    let target_url_str = if force_visible {
        "https://accounts.google.com/ServiceLogin?service=youtube"
    } else {
        "https://www.youtube.com"
    };

    let target_url = Url::parse(target_url_str).map_err(|e| e.to_string())?;
    let app_handle_clone = app.clone();
    let tx_nav = Arc::clone(&tx);
    let auth_nav = Arc::clone(&is_authenticated);

    let builder = WebviewWindowBuilder::new(
        &app,
        "youtube-login",
        WebviewUrl::External(target_url),
    )
        .data_directory(app.path().app_data_dir().unwrap().join("browser-profiles"))
        .title("Blacktape | Sign into YouTube")
        .inner_size(500.0, 600.0)
        .resizable(false)
        .always_on_top(true)
        .visible(force_visible)
        .focused(force_visible)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
        .initialization_script(r#"
        // Listen for internal state reports from frontend
        window.addEventListener("message", (event) => {
            if (event.data && event.data.type === "BLACKTAPE_COOKIE_EXPORT") {
                window.location.href = "tauri://save-cookies?data=" + encodeURIComponent(event.data.cookies);
            }
        });
    "#)
        .on_navigation(move |url| {
            println!("[blacktape::auth] Navigating to: {}", url);
            let host = url.host_str();

            // 1. Intercept cookie save request after successful auth check
            if url.scheme() == "tauri" && host == Some("save-cookies") {
                let app_handle_task = app_handle_clone.clone();
                let tx_task = Arc::clone(&tx_nav);
                let auth_task = Arc::clone(&auth_nav);

                let query_str = url.query().unwrap_or("");
                let cookies_encoded = query_str.replace("data=", "");
                let clean_cookies = percent_encoding::percent_decode_str(&cookies_encoded)
                    .decode_utf8_lossy()
                    .to_string();

                tauri::async_runtime::spawn(async move {
                    println!("[blacktape::auth] Saving authenticated session cookies...");
                    if let Err(e) = write_netscape_cookies(&app_handle_task, &clean_cookies).await {
                        eprintln!("[blacktape::auth ERROR] Failed to write cookies: {}", e);
                    }

                    // Mark auth as successful before closing window
                    auth_task.store(true, Ordering::SeqCst);

                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        let _ = win.close();
                    }

                    if let Ok(mut guard) = tx_task.lock() {
                        if let Some(sender) = guard.take() {
                            let _ = sender.send(());
                        }
                    }
                });
                return false;
            }

            // 2. Unhide window if user lands on Google Accounts login
            if host == Some("accounts.google.com") {
                println!("[blacktape::auth] Login required. Showing window to user...");
                if let Some(win) = app_handle_clone.get_webview_window("youtube-login") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }

            // 3. User arrived back on YouTube post-login: Verify session state
            if host == Some("www.youtube.com") {
                let app_handle_task = app_handle_clone.clone();
                tauri::async_runtime::spawn(async move {
                    // Delay slightly to let cookies write to DOM
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        let js = r#"
                        (function() {
                            const cookies = document.cookie;
                            // Check for LOGIN_INFO or logged in state
                            const isLoggedIn = cookies.includes("LOGIN_INFO=") ||
                                               (window.yt && window.yt.config_ && window.yt.config_.LOGGED_IN === true);

                            if (isLoggedIn) {
                                window.postMessage({ type: 'BLACKTAPE_COOKIE_EXPORT', cookies: cookies }, '*');
                            } else if (window.location.href.includes("youtube.com")) {
                                console.log("[blacktape] Not logged in yet. Redirecting to Google Login...");
                                window.location.href = "https://accounts.google.com/ServiceLogin?service=youtube";
                            }
                        })();
                    "#;
                        let _ = win.eval(js);
                    }
                });
            }

            true
        });

    let login_window = builder.build().map_err(|e| e.to_string())?;

    // Handle window closure: If closed before auth, cancel download
    let tx_close = Arc::clone(&tx);
    login_window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            if let Ok(mut guard) = tx_close.lock() {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(());
                }
            }
        }
    });

    // Pause until window is closed or cookies are saved
    let _ = rx.await;

    // Check auth flag: If user manually closed window without logging in, CANCEL!
    if !is_authenticated.load(Ordering::SeqCst) {
        println!("[blacktape::auth] Window closed without valid authentication. Canceling batch!");
        return Err("Authentication canceled by user.".to_string());
    }

    println!("[blacktape::auth] Authentication verified. Proceeding with download batch.");
    Ok(())
}

#[command]
pub async fn check_auth_status(app: AppHandle) -> bool {
    let mut cookie_file = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    cookie_file.push("youtube_cookies.txt");

    if !cookie_file.exists() {
        return false;
    }

    let contents = match read_to_string(cookie_file).await {
        Ok(text) => text,
        Err(_) => return false,
    };

    let has_sid = contents.contains("SID");
    let has_sapisid = contents.contains("SAPISID");

    has_sid && has_sapisid
}

async fn write_netscape_cookies(app: &AppHandle, cookies_str: &str) -> Result<(), String> {
    println!(
        "[blacktape::auth] write_netscape_cookies invoked with string length: {}",
        cookies_str.len()
    );

    let mut cookie_file_path = app.path().app_data_dir().map_err(|e| {
        eprintln!("[blacktape::auth ERROR] Failed to get app_data_dir: {}", e);
        e.to_string()
    })?;
    cookie_file_path.push("youtube_cookies.txt");

    let file = File::create(&cookie_file_path).await.map_err(|e| {
        eprintln!(
            "[blacktape::auth ERROR] Failed to create cookie file: {}",
            e
        );
        e.to_string()
    })?;

    let mut writer = BufWriter::new(file);

    writer
        .write_all(b"# Netscape HTTP Cookie File\n")
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(b"# This file was generated by Blacktape. Do not edit.\n\n")
        .await
        .map_err(|e| e.to_string())?;

    let mut cookie_count = 0;
    for cookie in cookies_str.split(';') {
        let parts: Vec<&str> = cookie.trim().splitn(2, '=').collect();
        if parts.len() == 2 {
            let name = parts[0];
            let value = parts[1];
            cookie_count += 1;

            let line = format!(".youtube.com\tTRUE\t/\tTRUE\t0\t{}\t{}\n", name, value);
            writer.write_all(line.as_bytes()).await.map_err(|e| {
                eprintln!("[blacktape::auth ERROR] Failed writing cookie line: {}", e);
                e.to_string()
            })?;
        }
    }
    println!(
        "[blacktape::auth] Parsed and prepared {} cookies to write.",
        cookie_count
    );

    writer.flush().await.map_err(|e| {
        eprintln!("[blacktape::auth ERROR] Failed to flush BufWriter: {}", e);
        e.to_string()
    })?;

    println!(
        "[blacktape::auth SUCCESS] Authenticated yt-dlp cookies saved to: {:?}",
        cookie_file_path
    );
    Ok(())
}
