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

#[command]
pub async fn launch_youtube_login(app: AppHandle) -> Result<(), String> {
    println!("[blacktape::auth] launch_youtube_login command invoked.");

    let login_url_str = "https://accounts.google.com/ServiceLogin?service=youtube";
    let target_url = Url::parse(login_url_str).map_err(|e| e.to_string())?;

    let app_handle_clone = app.clone();

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
        // 1. Inject a secure window listener that listens ONLY for an internal browser message
        .initialization_script(r#"
        window.addEventListener("message", (event) => {
            if (event.data && event.data.type === "BLACKTAPE_EXTRACT_COOKIES") {
                // Send it back via a standard message structure that Tauri handles internally
                console.log("[blacktape frontend] Extraction triggered. Sending cookies up...");
                window.location.href = "tauri://cookies?data=" + encodeURIComponent(document.cookie);
            }
        });
    "#)
        .on_navigation(move |url| {
            println!("[blacktape::auth] Navigation detected to: {}", url);

            // 2. Catch our custom protocol redirect containing the cookies!
            if url.scheme() == "tauri" && url.host_str() == Some("cookies") {
                let app_handle_task = app_handle_clone.clone();

                // Extract the query parameter from our custom URI redirection
                let query_str = url.query().unwrap_or("");
                let cookies_encoded = query_str.replace("data=", "");
                let clean_cookies = percent_encoding::percent_decode_str(&cookies_encoded)
                    .decode_utf8_lossy()
                    .to_string();

                tauri::async_runtime::spawn(async move {
                    println!("[blacktape::auth] Custom URI intercepted! Writing cookies...");
                    if let Err(e) = write_netscape_cookies(&app_handle_task, &clean_cookies).await {
                        eprintln!("[blacktape::auth ERROR] Failed to write cookie file: {}", e);
                    }

                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        println!("[blacktape::auth] Successfully stored cookies. Closing login window.");
                        let _ = win.close();
                    }
                });
                return false; // Stop navigation here so it doesn't actually try to route to "tauri://cookies"
            }

            let host = url.host_str();
            let path = url.path();

            if host == Some("www.youtube.com") && (path == "/" || path.is_empty()) {
                println!("[blacktape::auth] YouTube landing detected! Checking for extraction trigger...");

                let app_handle_task = app_handle_clone.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(win) = app_handle_task.get_webview_window("youtube-login") {
                        println!("[blacktape::auth] Dispatching collection trigger via postMessage...");
                        // Safely triggers the initialization script handler
                        let _ = win.eval("window.postMessage({ type: 'BLACKTAPE_EXTRACT_COOKIES' }, '*');");
                    }
                });
            }
            true
        });

    let _login_window = builder.build().map_err(|e| e.to_string())?;

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
