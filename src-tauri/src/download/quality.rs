//! Download quality: what a platform is expected to give (shown at once, no yt-dlp run) and the audio formats
//! yt-dlp really finds for a track (a 3-6 s probe, run when the user opens the quality menu).

use crate::types::Platform;
use serde::Serialize;
use serde_json::Value;

/// One audio format yt-dlp can download for a track.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioFormat {
    /// yt-dlp format id ("251", "flac", "mp3-320", "hls_aac_160k"); passed back as the download's `format`.
    pub id: String,
    /// "Opus", "AAC", "MP3", "FLAC", "ALAC", "WAV", "AIFF", "Vorbis" or the codec as yt-dlp names it.
    pub codec: String,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub lossless: bool,
    /// What the menu shows: "FLAC (lossless)", "MP3 320 kbps", "MP3 V0 (~245 kbps)", "Opus 131 kbps".
    pub label: String,
}

/// The quality a platform is expected to give, shown before any probe.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpectedQuality {
    pub label: String,
    pub lossless: bool,
}

/// Expected quality of a download from `platform`. `lossless_available`: a Bandcamp album that is free / name your
/// price or bought on the user's account (other platforms ignore it). Pure.
pub fn expected_quality(platform: Platform, lossless_available: bool) -> ExpectedQuality {
    let (label, lossless) = match platform {
        Platform::Youtube => ("Opus ~160 kbps", false),
        // a direct MP3 when the track still has one (what the download prefers), else AAC 160 kbps over HLS
        Platform::Soundcloud => ("MP3 128 / AAC 160 kbps", false),
        Platform::Bandcamp if lossless_available => ("FLAC (lossless)", true),
        Platform::Bandcamp => ("MP3 128 kbps", false),
        Platform::Local => ("Local file", false),
    };
    ExpectedQuality { label: label.to_string(), lossless }
}

/// True when a Bandcamp album or track page offers a free download (free or "name your price"): its
/// `data-tralbum` has a `freeDownloadPage` url instead of `null`. Pure.
pub fn bandcamp_has_free_download(html: &str) -> bool {
    ["freeDownloadPage&quot;:&quot;", "\"freeDownloadPage\":\"", "\"freeDownloadPage\": \""]
        .iter()
        .any(|needle| html.contains(needle))
}

/// True when the album is in the user's Bandcamp collection (bought). Bandcamp accounts are not supported yet, so
/// this is always false; once they are, the downloads get the purchase's lossless files too.
pub async fn bandcamp_purchased(_url: &str) -> bool {
    false
}

/// A format id that is safe to hand to yt-dlp's `-f` (letters, digits, `_`, `-`, `.`; no selector syntax such as
/// `/`, `+`, `[`). Pure.
pub fn checked_format_id(id: &str) -> Option<&str> {
    let id = id.trim();
    let ok = !id.is_empty()
        && id.len() <= 40
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    ok.then_some(id)
}

/// `args` with `preferred` tried first by the `-f` selector (the platform's own chain stays as the fallback, so a
/// track that lacks the chosen format still downloads in the next best one). Unchanged without `preferred`, for an
/// unsafe id, or when there is no `-f`. Pure.
pub fn prefer_format(mut args: Vec<String>, preferred: Option<&str>) -> Vec<String> {
    let Some(id) = preferred.and_then(checked_format_id) else {
        return args;
    };
    if let Some(i) = args.iter().position(|a| a == "-f") {
        if let Some(chain) = args.get_mut(i + 1) {
            *chain = format!("{id}/{chain}");
        }
    }
    args
}

fn codec_name(acodec: &str, ext: &str, id: &str) -> String {
    let a = acodec.to_ascii_lowercase();
    let key = if a.is_empty() || a == "none" || a == "unknown" { ext.to_ascii_lowercase() } else { a };
    let name = match key.as_str() {
        k if k.starts_with("opus") => "Opus",
        k if k.starts_with("mp4a") || k.starts_with("aac") || k == "m4a" && !id.contains("alac") => "AAC",
        k if k.starts_with("mp3") => "MP3",
        k if k.starts_with("flac") => "FLAC",
        k if k.starts_with("alac") => "ALAC",
        k if k.starts_with("vorbis") || k == "ogg" => "Vorbis",
        k if k.starts_with("pcm") || k == "wav" => "WAV",
        k if k.starts_with("aiff") => "AIFF",
        _ => return key.to_uppercase(),
    };
    name.to_string()
}

/// Bitrate written in the format id: "mp3-320" → 320, "hls_aac_160k" → 160. A plain number is a YouTube itag
/// ("140"), not a bitrate. Pure.
fn bitrate_in_id(id: &str) -> Option<u32> {
    if id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    id.split(['-', '_'])
        .filter_map(|part| part.trim_end_matches('k').parse::<u32>().ok())
        .find(|n| (32..=1411).contains(n))
}

fn label_of(codec: &str, bitrate: Option<u32>, lossless: bool, id: &str) -> String {
    if lossless {
        return format!("{codec} (lossless)");
    }
    if id.eq_ignore_ascii_case("mp3-v0") {
        return "MP3 V0 (~245 kbps)".to_string();
    }
    // Bandcamp's download-file encodes: high-quality VBR, better than its MP3 128 stream
    if bitrate.is_none() && (id.eq_ignore_ascii_case("aac-hi") || id.eq_ignore_ascii_case("vorbis")) {
        return format!("{codec} (high quality)");
    }
    match bitrate {
        Some(kbps) => format!("{codec} {kbps} kbps"),
        None => codec.to_string(),
    }
}

/// The audio formats of yt-dlp's `-J` output (`formats`), best first: lossless, then by bitrate. Formats with video
/// are only listed when the track has no audio-only format (an age-gated YouTube video offers only a 360p mp4).
/// Pure.
pub fn audio_formats(info: &Value) -> Vec<AudioFormat> {
    let Some(formats) = info.get("formats").and_then(Value::as_array) else {
        return Vec::new();
    };
    let str_of = |f: &Value, key: &str| f.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let has_audio = |f: &Value| !matches!(f.get("acodec").and_then(Value::as_str), Some("none"));
    let audio_only = |f: &Value| matches!(f.get("vcodec").and_then(Value::as_str), Some("none") | None);
    let with_audio: Vec<&Value> = formats.iter().filter(|f| has_audio(f) && !str_of(f, "format_id").is_empty()).collect();
    let only: Vec<&Value> = with_audio.iter().copied().filter(|f| audio_only(f)).collect();
    let mut chosen = if only.is_empty() { with_audio } else { only };
    // the same quality as a direct file and as an HLS stream (SoundCloud): the direct file is kept
    chosen.sort_by_key(|f| str_of(f, "protocol").contains("m3u8"));

    let mut out: Vec<AudioFormat> = chosen
        .into_iter()
        .map(|f| {
            let id = str_of(f, "format_id");
            let codec = codec_name(&str_of(f, "acodec"), &str_of(f, "ext"), &id);
            let lossless = matches!(codec.as_str(), "FLAC" | "ALAC" | "WAV" | "AIFF");
            let bitrate = if lossless {
                None
            } else {
                f.get("abr")
                    .and_then(Value::as_f64)
                    .filter(|b| *b > 0.0)
                    .map(|b| b.round() as u32)
                    .or_else(|| bitrate_in_id(&id))
            };
            let sample_rate = f.get("asr").and_then(Value::as_u64).and_then(|r| u32::try_from(r).ok());
            AudioFormat { label: label_of(&codec, bitrate, lossless, &id), id, codec, bitrate_kbps: bitrate, sample_rate_hz: sample_rate, lossless }
        })
        .collect();
    let rank = |f: &AudioFormat| {
        let lossless_rank = match f.codec.as_str() {
            "FLAC" => 4,
            "ALAC" => 3,
            "WAV" => 2,
            "AIFF" => 1,
            _ => 0,
        };
        // formats without a bitrate in their id: ranked by what Bandcamp encodes them at (below MP3 V0, above
        // MP3 128)
        let vbr = match f.id.to_ascii_lowercase().as_str() {
            "mp3-v0" => 245,
            "aac-hi" => 180,
            "vorbis" => 160,
            _ => 0,
        };
        let kbps = f.bitrate_kbps.unwrap_or(vbr);
        // Bitrates of different codecs are not comparable: Opus at ~100 kbps sounds as good as AAC at ~130-150 and
        // MP3 needs more still. Rank by an MP3-equivalent bitrate (percent weights from listening tests).
        let weight = match f.codec.as_str() {
            "Opus" => 150,
            "Vorbis" => 110,
            "AAC" => 100,
            "MP3" => 75,
            _ => 100,
        };
        (lossless_rank, kbps * weight)
    };
    out.sort_by(|a, b| rank(b).cmp(&rank(a)));
    // one row per quality the user can tell apart
    let mut seen = std::collections::HashSet::new();
    out.retain(|f| seen.insert(f.label.clone()));
    out
}

/// yt-dlp's arguments to read the formats of one track (no download). The url is checked and comes last, after
/// `--`. Pure.
pub fn probe_args(url: &str, cookie_arg: Option<&str>) -> Result<Vec<String>, String> {
    let url = crate::download::ytdlp::checked_url(url)?;
    let mut args: Vec<String> =
        ["-J", "--skip-download", "--no-playlist", "--no-warnings"].iter().map(|s| s.to_string()).collect();
    if let Some(cookie) = cookie_arg {
        args.push("--cookies-from-browser".to_string());
        args.push(cookie.to_string());
    }
    args.push("--".to_string());
    args.push(url.to_string());
    Ok(args)
}

/// Expected quality of the platform album or track at `url` (Bandcamp reads its page, about 1 s; the others answer
/// at once). A page that cannot be read counts as "no free download".
#[tauri::command]
pub async fn get_expected_quality(platform: Platform, url: String) -> ExpectedQuality {
    let lossless = platform == Platform::Bandcamp && {
        let free = match reqwest::get(url.trim()).await {
            Ok(response) => response.text().await.is_ok_and(|html| bandcamp_has_free_download(&html)),
            Err(_) => false,
        };
        free || bandcamp_purchased(&url).await
    };
    expected_quality(platform, lossless)
}

/// The first track page of the album at `url`.
async fn first_track_url(platform: Platform, url: &str) -> Result<String, String> {
    let album = match platform {
        Platform::Bandcamp => crate::download::bandcamp::parse_album(url).await?,
        Platform::Youtube => crate::download::youtube::parse_album(url).await?,
        Platform::Soundcloud => crate::download::soundcloud::parse_album(url).await?,
        Platform::Local => return Err("Local files have no download formats.".into()),
    };
    album
        .tracks
        .into_iter()
        .map(|t| t.url)
        .find(|u| !u.trim().is_empty())
        .ok_or_else(|| "The album has no downloadable track.".to_string())
}

/// The audio formats yt-dlp finds for the track at `url`, or for the first track of the album at `url`
/// (`track_url` skips reading the album when the caller knows a track). Takes about 3-6 s.
#[tauri::command]
pub async fn probe_audio_formats(
    app: tauri::AppHandle,
    platform: Platform,
    url: String,
    is_album: bool,
    track_url: Option<String>,
) -> Result<Vec<AudioFormat>, String> {
    use tauri::Manager;
    let track = match track_url.filter(|u| !u.trim().is_empty()) {
        Some(track) => track,
        None if is_album => first_track_url(platform, &url).await?,
        None => url,
    };
    // the same browser profile the downloads use, so a signed-in account shows the formats it unlocks
    let profile = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("browser-profiles")
        .join("EBWebView")
        .join("Default");
    let cookie = (platform != Platform::Soundcloud && profile.exists())
        .then(|| format!("edge:{}", profile.to_string_lossy()));
    let args = probe_args(&track, cookie.as_deref())?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = crate::download::ytdlp::execute(&app, &refs).await?;
    let info: Value = serde_json::from_str(&out).map_err(|e| format!("yt-dlp gave no format list: {e}"))?;
    let formats = audio_formats(&info);
    if formats.is_empty() {
        return Err("No audio formats were found for this track.".into());
    }
    Ok(formats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An audio-only yt-dlp format as `-J` lists it.
    fn audio(id: &str, ext: &str, acodec: Option<&str>, abr: Option<f64>, asr: Option<u64>) -> Value {
        json!({
            "format_id": id,
            "ext": ext,
            "acodec": acodec,
            "vcodec": "none",
            "abr": abr,
            "asr": asr,
        })
    }

    fn ids(formats: &[AudioFormat]) -> Vec<&str> {
        formats.iter().map(|f| f.id.as_str()).collect()
    }

    fn labels(formats: &[AudioFormat]) -> Vec<&str> {
        formats.iter().map(|f| f.label.as_str()).collect()
    }

    fn chain() -> Vec<String> {
        ["-f", "251/140/ba", "-o", "x"].map(String::from).to_vec()
    }

    // ---- expected_quality ----

    #[test]
    fn expected_quality_per_platform() {
        assert_eq!(
            expected_quality(Platform::Youtube, false),
            ExpectedQuality { label: "Opus ~160 kbps".into(), lossless: false }
        );
        assert_eq!(
            expected_quality(Platform::Soundcloud, false),
            ExpectedQuality { label: "MP3 128 / AAC 160 kbps".into(), lossless: false }
        );
        assert_eq!(
            expected_quality(Platform::Local, false),
            ExpectedQuality { label: "Local file".into(), lossless: false }
        );
    }

    #[test]
    fn expected_quality_of_bandcamp_depends_on_lossless_availability() {
        assert_eq!(
            expected_quality(Platform::Bandcamp, true),
            ExpectedQuality { label: "FLAC (lossless)".into(), lossless: true }
        );
        assert_eq!(
            expected_quality(Platform::Bandcamp, false),
            ExpectedQuality { label: "MP3 128 kbps".into(), lossless: false }
        );
    }

    #[test]
    fn expected_quality_ignores_lossless_availability_off_bandcamp() {
        assert!(!expected_quality(Platform::Youtube, true).lossless);
        assert!(!expected_quality(Platform::Soundcloud, true).lossless);
        assert_eq!(expected_quality(Platform::Youtube, true).label, "Opus ~160 kbps");
    }

    // ---- bandcamp_has_free_download ----

    #[test]
    fn free_download_page_in_escaped_data_tralbum_counts() {
        let html = r#"<script data-tralbum="{&quot;freeDownloadPage&quot;:&quot;https://a.bandcamp.com/download?x&quot;}"></script>"#;
        assert!(bandcamp_has_free_download(html));
    }

    #[test]
    fn free_download_page_in_plain_json_counts_with_or_without_space() {
        assert!(bandcamp_has_free_download(r#"{"freeDownloadPage":"https://a.bandcamp.com/download"}"#));
        assert!(bandcamp_has_free_download(r#"{"freeDownloadPage": "https://a.bandcamp.com/download"}"#));
    }

    #[test]
    fn null_free_download_page_does_not_count() {
        assert!(!bandcamp_has_free_download("freeDownloadPage&quot;:null"));
        assert!(!bandcamp_has_free_download(r#"{"freeDownloadPage":null}"#));
    }

    #[test]
    fn page_without_the_key_has_no_free_download() {
        assert!(!bandcamp_has_free_download(""));
        assert!(!bandcamp_has_free_download("<html>buy the album</html>"));
    }

    // ---- checked_format_id ----

    #[test]
    fn checked_format_id_accepts_yt_dlp_ids() {
        assert_eq!(checked_format_id("251"), Some("251"));
        assert_eq!(checked_format_id("mp3-320"), Some("mp3-320"));
        assert_eq!(checked_format_id("hls_aac_160k"), Some("hls_aac_160k"));
        assert_eq!(checked_format_id("a.b_c-d"), Some("a.b_c-d"));
    }

    #[test]
    fn checked_format_id_trims_surrounding_spaces() {
        assert_eq!(checked_format_id("  140 "), Some("140"));
    }

    #[test]
    fn checked_format_id_refuses_selector_syntax_and_empty_ids() {
        for bad in ["251/best", "a+b", "", "   ", "a[ext=m4a]", "a,b", "a b"] {
            assert_eq!(checked_format_id(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn checked_format_id_length_limit_is_40() {
        assert!(checked_format_id(&"a".repeat(40)).is_some());
        assert!(checked_format_id(&"a".repeat(41)).is_none());
    }

    // ---- prefer_format ----

    #[test]
    fn prefer_format_puts_the_chosen_id_first_in_the_selector() {
        let args = prefer_format(chain(), Some("140"));
        assert_eq!(args, ["-f", "140/251/140/ba", "-o", "x"]);
    }

    #[test]
    fn prefer_format_trims_the_id_it_uses() {
        let args = prefer_format(chain(), Some(" 140 "));
        assert_eq!(args[1], "140/251/140/ba");
    }

    #[test]
    fn prefer_format_without_a_choice_or_with_an_unsafe_id_changes_nothing() {
        assert_eq!(prefer_format(chain(), None), chain());
        for bad in ["251/best", "a+b", ""] {
            assert_eq!(prefer_format(chain(), Some(bad)), chain(), "changed for {bad:?}");
        }
    }

    #[test]
    fn prefer_format_leaves_args_without_a_selector_alone() {
        let args: Vec<String> = ["-o", "x"].map(String::from).to_vec();
        assert_eq!(prefer_format(args.clone(), Some("140")), args);
    }

    #[test]
    fn prefer_format_does_not_panic_on_a_dangling_f_flag() {
        let args: Vec<String> = ["-f"].map(String::from).to_vec();
        assert_eq!(prefer_format(args.clone(), Some("140")), args);
    }

    // ---- audio_formats: YouTube ----

    fn youtube_info() -> Value {
        json!({
            "formats": [
                {"format_id": "139", "ext": "m4a", "acodec": "mp4a.40.5", "vcodec": "none", "abr": 48.898, "asr": 22050},
                {"format_id": "249", "ext": "webm", "acodec": "opus", "vcodec": "none", "abr": 50.822, "asr": 48000},
                {"format_id": "250", "ext": "webm", "acodec": "opus", "vcodec": "none", "abr": 66.752, "asr": 48000},
                {"format_id": "140", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 129.635, "asr": 44100},
                {"format_id": "251", "ext": "webm", "acodec": "opus", "vcodec": "none", "abr": 132.669, "asr": 48000},
                {"format_id": "18", "ext": "mp4", "acodec": "mp4a.40.2", "vcodec": "avc1", "abr": null, "asr": 22050}
            ]
        })
    }

    #[test]
    fn youtube_formats_are_audio_only_best_bitrate_first() {
        let formats = audio_formats(&youtube_info());
        assert_eq!(ids(&formats), ["251", "140", "250", "249", "139"]);
        assert_eq!(
            labels(&formats),
            ["Opus 133 kbps", "AAC 130 kbps", "Opus 67 kbps", "Opus 51 kbps", "AAC 49 kbps"]
        );
    }

    #[test]
    fn youtube_video_format_is_left_out_when_audio_only_formats_exist() {
        let formats = audio_formats(&youtube_info());
        assert!(!ids(&formats).contains(&"18"));
    }

    #[test]
    fn youtube_formats_carry_codec_rate_and_sample_rate() {
        let formats = audio_formats(&youtube_info());
        let best = &formats[0];
        assert_eq!(best.codec, "Opus");
        assert_eq!(best.bitrate_kbps, Some(133));
        assert_eq!(best.sample_rate_hz, Some(48000));
        assert!(!best.lossless);
        let aac = formats.iter().find(|f| f.id == "140").unwrap();
        assert_eq!(aac.codec, "AAC");
        assert_eq!(aac.sample_rate_hz, Some(44100));
    }

    // ---- audio_formats: Bandcamp free album ----

    fn bandcamp_free_info() -> Value {
        json!({
            "formats": [
                audio("mp3-128", "mp3", Some("mp3"), None, None),
                audio("mp3-v0", "mp3", Some("mp3"), None, None),
                audio("mp3-320", "mp3", Some("mp3"), None, None),
                audio("aac-hi", "m4a", Some("aac"), None, None),
                audio("vorbis", "ogg", Some("vorbis"), None, None),
                audio("aiff-lossless", "aiff", Some("aiff"), None, None),
                audio("wav", "wav", Some("pcm_s16le"), None, None),
                audio("flac", "flac", Some("flac"), None, None),
                audio("falac", "m4a", Some("alac"), None, None),
            ]
        })
    }

    #[test]
    fn bandcamp_free_album_lists_lossless_first_then_mp3_by_bitrate() {
        let formats = audio_formats(&bandcamp_free_info());
        assert_eq!(
            ids(&formats),
            ["flac", "falac", "wav", "aiff-lossless", "mp3-320", "mp3-v0", "aac-hi", "vorbis", "mp3-128"]
        );
        assert_eq!(
            labels(&formats),
            [
                "FLAC (lossless)",
                "ALAC (lossless)",
                "WAV (lossless)",
                "AIFF (lossless)",
                "MP3 320 kbps",
                "MP3 V0 (~245 kbps)",
                "AAC (high quality)",
                "Vorbis (high quality)",
                "MP3 128 kbps",
            ]
        );
    }

    #[test]
    fn bandcamp_lossless_formats_are_marked_lossless_and_the_rest_are_not() {
        let formats = audio_formats(&bandcamp_free_info());
        let lossless: Vec<bool> = formats.iter().map(|f| f.lossless).collect();
        assert_eq!(lossless, [true, true, true, true, false, false, false, false, false]);
    }

    #[test]
    fn bandcamp_bitrate_is_read_from_the_format_id() {
        let formats = audio_formats(&bandcamp_free_info());
        let bitrate = |id: &str| formats.iter().find(|f| f.id == id).unwrap().bitrate_kbps;
        assert_eq!(bitrate("mp3-320"), Some(320));
        assert_eq!(bitrate("mp3-128"), Some(128));
        assert_eq!(bitrate("flac"), None);
    }

    #[test]
    fn bandcamp_wav_is_recognised_from_its_extension_when_acodec_is_missing() {
        let info = json!({"formats": [audio("wav", "wav", None, None, None)]});
        let formats = audio_formats(&info);
        assert_eq!(formats[0].codec, "WAV");
        assert!(formats[0].lossless);
        assert_eq!(formats[0].label, "WAV (lossless)");
    }

    #[test]
    fn bandcamp_paid_album_has_only_mp3_128() {
        let info = json!({"formats": [audio("mp3-128", "mp3", Some("mp3"), None, None)]});
        let formats = audio_formats(&info);
        assert_eq!(labels(&formats), ["MP3 128 kbps"]);
        assert!(!formats[0].lossless);
    }

    // ---- audio_formats: SoundCloud ----

    #[test]
    fn soundcloud_hls_formats_are_sorted_by_abr() {
        let info = json!({"formats": [
            audio("hls_aac_96k", "m4a", Some("mp4a.40.2"), Some(96.0), None),
            audio("hls_aac_160k", "m4a", Some("mp4a.40.2"), Some(160.0), None),
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["hls_aac_160k", "hls_aac_96k"]);
        assert_eq!(labels(&formats), ["AAC 160 kbps", "AAC 96 kbps"]);
    }

    #[test]
    fn soundcloud_direct_and_hls_of_the_same_quality_are_listed_once() {
        // the direct MP3 and its HLS twin have the same label: only the direct file is listed
        let info = json!({"formats": [
            {"format_id": "hls_mp3_128", "ext": "mp3", "acodec": "mp3", "vcodec": "none", "abr": 128.0, "protocol": "m3u8_native"},
            {"format_id": "http_mp3_128", "ext": "mp3", "acodec": "mp3", "vcodec": "none", "abr": 128.0, "protocol": "http"},
            {"format_id": "hls_aac_160k", "ext": "m4a", "acodec": "mp4a.40.2", "vcodec": "none", "abr": 160.0, "protocol": "m3u8_native"},
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["hls_aac_160k", "http_mp3_128"]);
        assert_eq!(labels(&formats), ["AAC 160 kbps", "MP3 128 kbps"]);
    }

    // ---- audio_formats: edge cases ----

    #[test]
    fn age_gated_video_only_info_still_lists_its_format() {
        let info = json!({"formats": [
            {"format_id": "18", "ext": "mp4", "acodec": "mp4a.40.2", "vcodec": "avc1", "abr": null, "asr": 22050}
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["18"]);
        assert_eq!(formats[0].codec, "AAC");
        assert_eq!(formats[0].sample_rate_hz, Some(22050));
    }

    #[test]
    fn info_without_formats_gives_an_empty_list() {
        assert!(audio_formats(&json!({"title": "no formats key"})).is_empty());
        assert!(audio_formats(&json!({"formats": []})).is_empty());
        assert!(audio_formats(&json!({"formats": "not a list"})).is_empty());
    }

    #[test]
    fn formats_without_an_id_are_skipped() {
        let info = json!({"formats": [audio("", "m4a", Some("mp4a.40.2"), Some(128.0), None)]});
        assert!(audio_formats(&info).is_empty());
    }

    #[test]
    fn formats_without_audio_are_skipped() {
        let info = json!({"formats": [
            {"format_id": "sb0", "ext": "mhtml", "acodec": "none", "vcodec": "none"}
        ]});
        assert!(audio_formats(&info).is_empty());
    }

    #[test]
    fn bitrate_from_the_id_is_used_when_abr_is_missing() {
        let info = json!({"formats": [audio("hls_aac_160k", "m4a", Some("mp4a.40.2"), None, None)]});
        let formats = audio_formats(&info);
        assert_eq!(formats[0].bitrate_kbps, Some(160));
        assert_eq!(formats[0].label, "AAC 160 kbps");
    }

    /// Known bug, not fixed here: a numeric yt-dlp id (an itag such as "140") is read as a bitrate when `abr` is
    /// missing, so the label says "AAC 140 kbps". Expected once fixed: no bitrate for that format.
    #[test]
    fn numeric_itag_id_is_not_read_as_a_bitrate() {
        let info = json!({"formats": [audio("140", "m4a", Some("mp4a.40.2"), None, None)]});
        assert_eq!(audio_formats(&info)[0].bitrate_kbps, None);
    }

    // ---- audio_formats: ranking across codecs ----

    #[test]
    fn youtube_opus_101_ranks_above_aac_130() {
        // AAC 130 * 100 = 13000 < Opus 101 * 150 = 15150
        let info = json!({"formats": [
            audio("140", "m4a", Some("mp4a.40.2"), Some(129.6), Some(44100)),
            audio("251", "webm", Some("opus"), Some(101.0), Some(48000)),
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["251", "140"]);
        assert_eq!(labels(&formats), ["Opus 101 kbps", "AAC 130 kbps"]);
    }

    #[test]
    fn aac_130_still_ranks_above_opus_73() {
        // AAC 130 * 100 = 13000 > Opus 73 * 150 = 10950: the weights do not make Opus always win
        let info = json!({"formats": [
            audio("opus73", "webm", Some("opus"), Some(73.0), Some(48000)),
            audio("aac130", "m4a", Some("mp4a.40.2"), Some(130.0), Some(44100)),
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["aac130", "opus73"]);
    }

    #[test]
    fn mp3_128_ranks_below_aac_128_and_opus_96() {
        // Opus 96 * 150 = 14400 > AAC 128 * 100 = 12800 > MP3 128 * 75 = 9600
        let info = json!({"formats": [
            audio("mp3-128", "mp3", Some("mp3"), Some(128.0), Some(44100)),
            audio("aac-128", "m4a", Some("mp4a.40.2"), Some(128.0), Some(44100)),
            audio("opus-96", "webm", Some("opus"), Some(96.0), Some(48000)),
        ]});
        let formats = audio_formats(&info);
        assert_eq!(ids(&formats), ["opus-96", "aac-128", "mp3-128"]);
        assert_eq!(labels(&formats), ["Opus 96 kbps", "AAC 128 kbps", "MP3 128 kbps"]);
    }

    // ---- probe_args ----

    #[test]
    fn probe_args_read_the_format_list_without_downloading() {
        let url = "https://artist.bandcamp.com/album/x";
        let args = probe_args(url, None).unwrap();
        assert_eq!(args, ["-J", "--skip-download", "--no-playlist", "--no-warnings", "--", url]);
    }

    #[test]
    fn probe_args_pass_the_browser_cookies_before_the_separator() {
        let args = probe_args("https://www.youtube.com/watch?v=abc", Some("edge:C:/profile")).unwrap();
        let cookies = args.iter().position(|a| a == "--cookies-from-browser").unwrap();
        assert_eq!(args[cookies + 1], "edge:C:/profile");
        let separator = args.iter().position(|a| a == "--").unwrap();
        assert!(cookies < separator);
        assert_eq!(args.last().unwrap(), "https://www.youtube.com/watch?v=abc");
    }

    #[test]
    fn probe_args_refuse_option_like_and_local_urls() {
        for bad in ["--exec=calc", "-x", "file:///C:/a.mp3", "ftp://x.com/a", ""] {
            assert!(probe_args(bad, None).is_err(), "accepted {bad:?}");
        }
    }
}
