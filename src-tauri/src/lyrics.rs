use serde::{Deserialize, Serialize};

const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const APP_USER_AGENT: &str = "Blacktape/0.3.0 (https://github.com/seafood/blacktape)";

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct LyricsSource {
    pub lyrics: String,
    pub source: String,
}

fn build_url(base: &str, params: &[(&str, &str)]) -> String {
    let mut url = base.to_string();
    if !params.is_empty() {
        url.push('?');
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(&urlencoding::encode(k));
            url.push('=');
            url.push_str(&urlencoding::encode(v));
        }
    }
    url
}

// ---------------------------------------------------------------------------
// Provider 1: LRCLIB (Priority 1)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LrclibTrack {
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

async fn fetch_lrclib(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: Option<u64>,
) -> Result<LyricsSource, String> {
    // 1. Direct match: /api/get
    let mut params = vec![("artist_name", artist), ("track_name", title)];
    let duration_str;
    if let Some(dur) = duration_secs {
        if dur > 0 {
            duration_str = dur.to_string();
            params.push(("duration", &duration_str));
        }
    }
    let album_val;
    if let Some(alb) = album {
        if !alb.trim().is_empty() && alb != "Unknown Album" {
            album_val = alb;
            params.push(("album_name", album_val));
        }
    }

    let get_url = build_url("https://lrclib.net/api/get", &params);
    let get_res = client
        .get(&get_url)
        .header("User-Agent", APP_USER_AGENT)
        .send()
        .await;

    if let Ok(res) = get_res {
        if res.status().is_success() {
            if let Ok(data) = res.json::<LrclibTrack>().await {
                if let Some(synced) = data.synced_lyrics {
                    if !synced.trim().is_empty() {
                        return Ok(LyricsSource {
                            lyrics: synced,
                            source: "https://lrclib.net".to_string(),
                        });
                    }
                }
                if let Some(plain) = data.plain_lyrics {
                    if !plain.trim().is_empty() {
                        return Ok(LyricsSource {
                            lyrics: plain,
                            source: "https://lrclib.net".to_string(),
                        });
                    }
                }
                if data.instrumental {
                    return Ok(LyricsSource {
                        lyrics: "[00:00.00] Instrumental".to_string(),
                        source: "https://lrclib.net".to_string(),
                    });
                }
            }
        }
    }

    // 2. Search fallback: /api/search
    let search_params = vec![("artist_name", artist), ("track_name", title)];
    let search_url = build_url("https://lrclib.net/api/search", &search_params);
    let search_res = client
        .get(&search_url)
        .header("User-Agent", APP_USER_AGENT)
        .send()
        .await;

    if let Ok(res) = search_res {
        if res.status().is_success() {
            if let Ok(results) = res.json::<Vec<LrclibTrack>>().await {
                // Prefer synced lyrics first
                for track in &results {
                    if let Some(synced) = &track.synced_lyrics {
                        if !synced.trim().is_empty() {
                            return Ok(LyricsSource {
                                lyrics: synced.clone(),
                                source: "https://lrclib.net".to_string(),
                            });
                        }
                    }
                }
                // Fallback to plain lyrics
                for track in &results {
                    if let Some(plain) = &track.plain_lyrics {
                        if !plain.trim().is_empty() {
                            return Ok(LyricsSource {
                                lyrics: plain.clone(),
                                source: "https://lrclib.net".to_string(),
                            });
                        }
                    }
                    if track.instrumental {
                        return Ok(LyricsSource {
                            lyrics: "[00:00.00] Instrumental".to_string(),
                            source: "https://lrclib.net".to_string(),
                        });
                    }
                }
            }
        }
    }

    Err("LRCLIB: no lyrics found".to_string())
}

// ---------------------------------------------------------------------------
// Provider 2: Musixmatch (Priority 2)
// ---------------------------------------------------------------------------

async fn get_musixmatch_token(client: &reqwest::Client) -> Result<String, String> {
    let url = "https://apic.musixmatch.com/ws/1.1/token.get?app_id=android-player-v1.0";
    let res = client
        .get(url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err("Musixmatch token request failed".to_string());
    }

    let val: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let token = val
        .get("message")
        .and_then(|m| m.get("body"))
        .and_then(|b| b.get("user_token"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Failed to parse Musixmatch user token".to_string())?;

    Ok(token.to_string())
}

pub fn clean_musixmatch_lyrics(text: &str) -> String {
    let mut cleaned = text;
    if let Some(pos) = cleaned.find("******* This Lyrics is NOT for Commercial use") {
        cleaned = &cleaned[..pos];
    } else if let Some(pos) = cleaned.find("*******") {
        cleaned = &cleaned[..pos];
    }
    cleaned.trim().to_string()
}

async fn fetch_musixmatch(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
    _duration_secs: Option<u64>,
) -> Result<LyricsSource, String> {
    let token = get_musixmatch_token(client)
        .await
        .unwrap_or_else(|_| "21052026ae37f14b2d6a5405e3ecbb11".to_string());

    // 1. Try matcher.subtitle.get (timed lyrics in LRC format)
    let subtitle_url = build_url(
        "https://apic.musixmatch.com/ws/1.1/matcher.subtitle.get",
        &[
            ("q_artist", artist),
            ("q_track", title),
            ("app_id", "android-player-v1.0"),
            ("usertoken", &token),
            ("subtitle_format", "lrc"),
        ],
    );
    let sub_res = client
        .get(&subtitle_url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await;

    if let Ok(res) = sub_res {
        if res.status().is_success() {
            if let Ok(val) = res.json::<serde_json::Value>().await {
                if let Some(body) = val
                    .get("message")
                    .and_then(|m| m.get("body"))
                    .and_then(|b| b.get("subtitle"))
                    .and_then(|s| s.get("subtitle_body"))
                    .and_then(|t| t.as_str())
                {
                    if !body.trim().is_empty() {
                        return Ok(LyricsSource {
                            lyrics: clean_musixmatch_lyrics(body),
                            source: "https://www.musixmatch.com".to_string(),
                        });
                    }
                }
            }
        }
    }

    // 2. Try matcher.lyrics.get (plain lyrics)
    let lyrics_url = build_url(
        "https://apic.musixmatch.com/ws/1.1/matcher.lyrics.get",
        &[
            ("q_artist", artist),
            ("q_track", title),
            ("app_id", "android-player-v1.0"),
            ("usertoken", &token),
        ],
    );
    let lyr_res = client
        .get(&lyrics_url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await;

    if let Ok(res) = lyr_res {
        if res.status().is_success() {
            if let Ok(val) = res.json::<serde_json::Value>().await {
                if let Some(body) = val
                    .get("message")
                    .and_then(|m| m.get("body"))
                    .and_then(|b| b.get("lyrics"))
                    .and_then(|s| s.get("lyrics_body"))
                    .and_then(|t| t.as_str())
                {
                    let cleaned = clean_musixmatch_lyrics(body);
                    if !cleaned.is_empty() {
                        return Ok(LyricsSource {
                            lyrics: cleaned,
                            source: "https://www.musixmatch.com".to_string(),
                        });
                    }
                }
            }
        }
    }

    // 3. Fallback: track.search to find track_id
    let search_url = build_url(
        "https://apic.musixmatch.com/ws/1.1/track.search",
        &[
            ("q_artist", artist),
            ("q_track", title),
            ("f_has_lyrics", "1"),
            ("page_size", "1"),
            ("app_id", "android-player-v1.0"),
            ("usertoken", &token),
        ],
    );
    let search_res = client
        .get(&search_url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await;

    if let Ok(res) = search_res {
        if res.status().is_success() {
            if let Ok(val) = res.json::<serde_json::Value>().await {
                let track = val
                    .get("message")
                    .and_then(|m| m.get("body"))
                    .and_then(|b| b.get("track_list"))
                    .and_then(|tl| tl.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|item| item.get("track"));

                if let Some(tr) = track {
                    let track_id = tr.get("track_id").and_then(|id| id.as_i64());
                    let track_share_url = tr
                        .get("track_share_url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("https://www.musixmatch.com")
                        .to_string();

                    if let Some(tid) = track_id {
                        let tid_str = tid.to_string();

                        // Try track.subtitle.get
                        let sub_url = build_url(
                            "https://apic.musixmatch.com/ws/1.1/track.subtitle.get",
                            &[
                                ("track_id", tid_str.as_str()),
                                ("app_id", "android-player-v1.0"),
                                ("usertoken", &token),
                                ("subtitle_format", "lrc"),
                            ],
                        );
                        let sub_res = client
                            .get(&sub_url)
                            .header("User-Agent", BROWSER_USER_AGENT)
                            .send()
                            .await;

                        if let Ok(sres) = sub_res {
                            if sres.status().is_success() {
                                if let Ok(sval) = sres.json::<serde_json::Value>().await {
                                    if let Some(sub) = sval
                                        .get("message")
                                        .and_then(|m| m.get("body"))
                                        .and_then(|b| b.get("subtitle"))
                                        .and_then(|s| s.get("subtitle_body"))
                                        .and_then(|t| t.as_str())
                                    {
                                        let cleaned = clean_musixmatch_lyrics(sub);
                                        if !cleaned.is_empty() {
                                            return Ok(LyricsSource {
                                                lyrics: cleaned,
                                                source: track_share_url,
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        // Try track.lyrics.get
                        let lyr_url = build_url(
                            "https://apic.musixmatch.com/ws/1.1/track.lyrics.get",
                            &[
                                ("track_id", tid_str.as_str()),
                                ("app_id", "android-player-v1.0"),
                                ("usertoken", &token),
                            ],
                        );
                        let lyr_res = client
                            .get(&lyr_url)
                            .header("User-Agent", BROWSER_USER_AGENT)
                            .send()
                            .await;

                        if let Ok(lres) = lyr_res {
                            if lres.status().is_success() {
                                if let Ok(lval) = lres.json::<serde_json::Value>().await {
                                    if let Some(lyr) = lval
                                        .get("message")
                                        .and_then(|m| m.get("body"))
                                        .and_then(|b| b.get("lyrics"))
                                        .and_then(|s| s.get("lyrics_body"))
                                        .and_then(|t| t.as_str())
                                    {
                                        let cleaned = clean_musixmatch_lyrics(lyr);
                                        if !cleaned.is_empty() {
                                            return Ok(LyricsSource {
                                                lyrics: cleaned,
                                                source: track_share_url,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err("Musixmatch: no lyrics found".to_string())
}

// ---------------------------------------------------------------------------
// Provider 3: lyrics.ovh (Priority 3)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LyricsResponse {
    lyrics: String,
}

#[derive(Deserialize)]
struct SuggestResponse {
    data: Vec<SongData>,
    #[allow(dead_code)]
    total: Option<usize>,
}

#[derive(Deserialize)]
struct SongData {
    title_short: String,
    artist: ArtistData,
}

#[derive(Deserialize)]
struct ArtistData {
    name: String,
}

async fn fetch_lyrics_ovh(
    client: &reqwest::Client,
    artist_input: &str,
    title_input: &str,
) -> Result<LyricsSource, String> {
    let suggest_url = format!(
        "https://api.lyrics.ovh/suggest/{}-{}",
        urlencoding::encode(artist_input),
        urlencoding::encode(title_input)
    );

    let mut clean_artist = artist_input.to_string();
    let mut clean_title = title_input.to_string();

    if let Ok(suggest_res) = client.get(&suggest_url).send().await {
        if suggest_res.status().is_success() {
            if let Ok(suggest_data) = suggest_res.json::<SuggestResponse>().await {
                if let Some(best_match) = suggest_data.data.first() {
                    clean_artist = best_match.artist.name.clone();
                    clean_title = best_match.title_short.clone();
                }
            }
        }
    }

    let lyrics_url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(&clean_artist),
        urlencoding::encode(&clean_title)
    );

    let lyrics_res = client
        .get(&lyrics_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if lyrics_res.status().is_success() {
        let data: LyricsResponse = lyrics_res.json().await.map_err(|e| e.to_string())?;
        if !data.lyrics.trim().is_empty() {
            return Ok(LyricsSource {
                lyrics: data.lyrics,
                source: "https://lyrics.ovh".to_string(),
            });
        }
    }

    // Direct match fallback
    if clean_artist != artist_input || clean_title != title_input {
        let direct_url = format!(
            "https://api.lyrics.ovh/v1/{}/{}",
            urlencoding::encode(artist_input),
            urlencoding::encode(title_input)
        );
        if let Ok(direct_res) = client.get(&direct_url).send().await {
            if direct_res.status().is_success() {
                if let Ok(data) = direct_res.json::<LyricsResponse>().await {
                    if !data.lyrics.trim().is_empty() {
                        return Ok(LyricsSource {
                            lyrics: data.lyrics,
                            source: "https://lyrics.ovh".to_string(),
                        });
                    }
                }
            }
        }
    }

    Err("lyrics.ovh: no lyrics found".to_string())
}

// ---------------------------------------------------------------------------
// Provider 4: Genius (Priority 4)
// ---------------------------------------------------------------------------

pub fn extract_genius_lyrics(html: &str) -> Option<String> {
    let mut lyrics_blocks = Vec::new();
    let container_attr = "data-lyrics-container=\"true\"";

    let mut search_idx = 0;
    while let Some(attr_pos) = html[search_idx..].find(container_attr) {
        let abs_attr_pos = search_idx + attr_pos;
        if let Some(_tag_open) = html[..abs_attr_pos].rfind("<div") {
            if let Some(rel_tag_close) = html[abs_attr_pos..].find('>') {
                let content_start = abs_attr_pos + rel_tag_close + 1;
                let mut depth = 1;
                let mut cur = content_start;
                let bytes = html.as_bytes();
                let len = bytes.len();
                let mut content_end = None;

                while cur < len {
                    if cur + 4 <= len && &html[cur..cur + 4] == "<div" {
                        let next_char = if cur + 4 < len { bytes[cur + 4] } else { b'>' };
                        if next_char.is_ascii_whitespace() || next_char == b'>' {
                            depth += 1;
                            cur += 4;
                            continue;
                        }
                    } else if cur + 6 <= len && &html[cur..cur + 6] == "</div>" {
                        depth -= 1;
                        if depth == 0 {
                            content_end = Some(cur);
                            break;
                        }
                        cur += 6;
                        continue;
                    }
                    cur += 1;
                }

                if let Some(end) = content_end {
                    let block_html = &html[content_start..end];
                    let with_newlines = block_html
                        .replace("<br/>", "\n")
                        .replace("<br />", "\n")
                        .replace("<br>", "\n");

                    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
                    let stripped = tag_re.replace_all(&with_newlines, "");
                    let decoded = html_escape::decode_html_entities(&stripped).to_string();
                    let trimmed = decoded.trim().to_string();
                    if !trimmed.is_empty() {
                        lyrics_blocks.push(trimmed);
                    }
                    search_idx = end + 6;
                    continue;
                }
            }
        }
        search_idx = abs_attr_pos + container_attr.len();
    }

    if lyrics_blocks.is_empty() {
        None
    } else {
        Some(lyrics_blocks.join("\n\n"))
    }
}

async fn fetch_genius(
    client: &reqwest::Client,
    artist: &str,
    title: &str,
) -> Result<LyricsSource, String> {
    let query = format!("{} {}", artist, title);
    let search_url = format!(
        "https://genius.com/api/search/multi?q={}",
        urlencoding::encode(&query)
    );

    let res = client
        .get(&search_url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err("Genius search failed".to_string());
    }

    let search_json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;

    let mut song_url: Option<String> = None;
    if let Some(sections) = search_json
        .get("response")
        .and_then(|r| r.get("sections"))
        .and_then(|s| s.as_array())
    {
        for section in sections {
            let sec_type = section.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if sec_type == "song" || sec_type == "top_hits" {
                if let Some(hits) = section.get("hits").and_then(|h| h.as_array()) {
                    for hit in hits {
                        if let Some(result) = hit.get("result") {
                            if let Some(url) = result.get("url").and_then(|u| u.as_str()) {
                                song_url = Some(url.to_string());
                                break;
                            }
                        }
                    }
                }
            }
            if song_url.is_some() {
                break;
            }
        }
    }

    let song_url = song_url.ok_or_else(|| "Genius: no song match found".to_string())?;

    let page_res = client
        .get(&song_url)
        .header("User-Agent", BROWSER_USER_AGENT)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !page_res.status().is_success() {
        return Err("Genius: failed to fetch song page".to_string());
    }

    let html = page_res.text().await.map_err(|e| e.to_string())?;
    let lyrics = extract_genius_lyrics(&html)
        .ok_or_else(|| "Genius: lyrics container not found".to_string())?;

    if lyrics.trim().is_empty() {
        return Err("Genius: empty lyrics".to_string());
    }

    Ok(LyricsSource {
        lyrics,
        source: song_url,
    })
}

// ---------------------------------------------------------------------------
// Helpers and Public Entry Point
// ---------------------------------------------------------------------------

pub fn clean_track_title(title: &str) -> String {
    let re = regex::Regex::new(
        r"(?i)\s*[\(\[](official\s*(music\s*)?video|official\s*audio|audio|visualizer|lyric\s*video|lyrics|hd|4k|remastered|remaster)[\)\]]",
    )
        .unwrap();
    let cleaned = re.replace_all(title, "");
    let feat_re = regex::Regex::new(r"(?i)\s*[\(\[](feat|ft)\.?\s+[^\)\]]+[\)\]]").unwrap();
    let cleaned = feat_re.replace_all(&cleaned, "");
    cleaned.trim().to_string()
}

/// Fetches lyrics attempting providers in priority order:
/// 1. lrclib
/// 2. musixmatch
/// 3. lyrics.ovh
/// 4. genius
pub async fn fetch_lyrics(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<LyricsSource, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let duration_secs = duration_ms.map(|d| d / 1000);
    let cleaned_title = clean_track_title(title);

    // 1. LRCLIB (Priority 1)
    if let Ok(res) = fetch_lrclib(&client, artist, title, album, duration_secs).await {
        return Ok(res);
    }
    if !cleaned_title.is_empty() && cleaned_title != title {
        if let Ok(res) = fetch_lrclib(&client, artist, &cleaned_title, album, duration_secs).await {
            return Ok(res);
        }
    }

    // 2. Musixmatch (Priority 2)
    if let Ok(res) = fetch_musixmatch(&client, artist, title, duration_secs).await {
        return Ok(res);
    }
    if !cleaned_title.is_empty() && cleaned_title != title {
        if let Ok(res) = fetch_musixmatch(&client, artist, &cleaned_title, duration_secs).await {
            return Ok(res);
        }
    }

    // 3. lyrics.ovh (Priority 3)
    if let Ok(res) = fetch_lyrics_ovh(&client, artist, title).await {
        return Ok(res);
    }
    if !cleaned_title.is_empty() && cleaned_title != title {
        if let Ok(res) = fetch_lyrics_ovh(&client, artist, &cleaned_title).await {
            return Ok(res);
        }
    }

    // 4. Genius (Priority 4)
    if let Ok(res) = fetch_genius(&client, artist, title).await {
        return Ok(res);
    }
    if !cleaned_title.is_empty() && cleaned_title != title {
        if let Ok(res) = fetch_genius(&client, artist, &cleaned_title).await {
            return Ok(res);
        }
    }

    Err("Lyrics not found across any provider".to_string())
}

/// Fetches lyrics from a specific provider ("lrclib", "musixmatch", "lyrics.ovh", "genius", or "auto")
pub async fn fetch_lyrics_from_provider(
    provider: &str,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<LyricsSource, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let duration_secs = duration_ms.map(|d| d / 1000);
    let cleaned_title = clean_track_title(title);

    match provider.trim().to_lowercase().as_str() {
        "lrclib" => {
            if let Ok(res) = fetch_lrclib(&client, artist, title, album, duration_secs).await {
                return Ok(res);
            }
            if !cleaned_title.is_empty() && cleaned_title != title {
                if let Ok(res) =
                    fetch_lrclib(&client, artist, &cleaned_title, album, duration_secs).await
                {
                    return Ok(res);
                }
            }
            Err("No lyrics found on LRCLIB".to_string())
        }
        "musixmatch" => {
            if let Ok(res) = fetch_musixmatch(&client, artist, title, duration_secs).await {
                return Ok(res);
            }
            if !cleaned_title.is_empty() && cleaned_title != title {
                if let Ok(res) =
                    fetch_musixmatch(&client, artist, &cleaned_title, duration_secs).await
                {
                    return Ok(res);
                }
            }
            Err("No lyrics found on Musixmatch".to_string())
        }
        "lyrics.ovh" | "lyrics_ovh" | "lyricsovh" => {
            if let Ok(res) = fetch_lyrics_ovh(&client, artist, title).await {
                return Ok(res);
            }
            if !cleaned_title.is_empty() && cleaned_title != title {
                if let Ok(res) = fetch_lyrics_ovh(&client, artist, &cleaned_title).await {
                    return Ok(res);
                }
            }
            Err("No lyrics found on lyrics.ovh".to_string())
        }
        "genius" => {
            if let Ok(res) = fetch_genius(&client, artist, title).await {
                return Ok(res);
            }
            if !cleaned_title.is_empty() && cleaned_title != title {
                if let Ok(res) = fetch_genius(&client, artist, &cleaned_title).await {
                    return Ok(res);
                }
            }
            Err("No lyrics found on Genius".to_string())
        }
        _ => fetch_lyrics(artist, title, album, duration_ms).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_track_title() {
        assert_eq!(
            clean_track_title("Never Gonna Give You Up (Official Music Video)"),
            "Never Gonna Give You Up"
        );
        assert_eq!(clean_track_title("Song Name [Official Audio]"), "Song Name");
        assert_eq!(clean_track_title("Track (feat. Another Artist)"), "Track");
        assert_eq!(
            clean_track_title("Track ft. Another Artist"),
            "Track ft. Another Artist"
        );
        assert_eq!(clean_track_title("Simple Song"), "Simple Song");
    }

    #[test]
    fn test_clean_musixmatch_lyrics() {
        let raw = "Here are the words\nTo the song\n\n******* This Lyrics is NOT for Commercial use *******\n(140961)";
        assert_eq!(
            clean_musixmatch_lyrics(raw),
            "Here are the words\nTo the song"
        );
    }

    #[test]
    fn test_extract_genius_lyrics() {
        let html = r#"
            <html>
            <body>
                <div class="something">Other content</div>
                <div data-lyrics-container="true" class="Lyrics__Container-1">
                    First line<br/>Second line with &amp; entity<br>Third line
                    <a href="/annotation"><span>annotated text</span></a>
                </div>
                <div data-lyrics-container="true" class="Lyrics__Container-2">
                    Chorus line 1<br/>Chorus line 2
                </div>
            </body>
            </html>
        "#;

        let extracted = extract_genius_lyrics(html).expect("lyrics should be extracted");
        assert!(extracted.contains("First line\nSecond line with & entity\nThird line"));
        assert!(extracted.contains("annotated text"));
        assert!(extracted.contains("Chorus line 1\nChorus line 2"));
    }
}
