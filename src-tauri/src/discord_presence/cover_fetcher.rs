use reqwest::blocking::Client;
use serde::Deserialize;
use std::path::Path;
use std::{collections::HashMap, fs, sync::Mutex};

use crate::types::Song;

const IMGBB_API_KEY: &str = "e5242a0d55eb6e3de9abe441dadb343e";

#[derive(Debug, Deserialize)]
struct MusicBrainzResponse {
    #[serde(default)]
    releases: Vec<MusicBrainzRelease>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzRelease {
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    title: String,
    #[serde(default)]
    score: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CoverArtArchiveResponse {
    #[serde(default)]
    pub images: Vec<CoverImage>,
}

#[derive(Debug, Deserialize)]
pub struct CoverImage {
    #[serde(default)]
    pub front: bool,
    #[serde(default)]
    pub approved: bool,
    pub thumbnails: Thumbnails,
}

#[derive(Debug, Deserialize)]
pub struct Thumbnails {
    pub large: String,
}

pub struct CoverFetcher {
    client: Client,
    cache: Mutex<HashMap<String, String>>,
}

impl CoverFetcher {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Blacktape/1.0 (xfefutu@gmail.com)")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn fetch_cover_url(&self, song: &Song) -> Option<String> {
        if let Some(cached_url) = self.cache.lock().unwrap().get(&song.path) {
            return Some(cached_url.clone());
        }

        if let Some(local_art_bytes) = self.extract_local_artwork(song) {
            if let Some(ref local_path_str) = song.cover_url {
                if let Some(hosted_url) =
                    self.upload_image_with_fallbacks(&local_art_bytes, local_path_str)
                {
                    self.cache
                        .lock()
                        .unwrap()
                        .insert(song.path.clone(), hosted_url.clone());
                    return Some(hosted_url);
                }
            }
        }

        let mbid = self.get_release_mbid(song)?;

        match self.get_cover_art_url(&mbid) {
            Ok(Some(url)) => {
                self.cache
                    .lock()
                    .unwrap()
                    .insert(song.path.clone(), url.clone());
                Some(url)
            }
            Ok(None) => None,
            Err(_) => None,
        }
    }

    fn extract_local_artwork(&self, song: &Song) -> Option<Vec<u8>> {
        if let Some(ref local_path_str) = song.cover_url {
            if !local_path_str.starts_with("http://") && !local_path_str.starts_with("https://") {
                let path = Path::new(local_path_str);
                if path.exists() && path.is_file() {
                    if let Ok(bytes) = fs::read(path) {
                        return Some(bytes);
                    }
                }
            }
        }

        None
    }

    fn upload_image_with_fallbacks(&self, image_bytes: &[u8], cover_path: &str) -> Option<String> {
        if image_bytes.is_empty() || image_bytes.len() < 100 {
            return None;
        }

        // Provider 1: ImgBB
        if let Some(url) = self.upload_to_imgbb(image_bytes) {
            return Some(url);
        }

        // Provider 2: Catbox.moe
        if let Some(url) = self.upload_to_catbox(image_bytes, cover_path) {
            return Some(url);
        }

        // Provider 3: 0x0.st
        if let Some(url) = self.upload_to_0x0(image_bytes, cover_path) {
            return Some(url);
        }

        // Provider 4: envs.sh
        if let Some(url) = self.upload_to_envs(image_bytes, cover_path) {
            return Some(url);
        }

        None
    }

    fn parse_imgbb_response(body: &str) -> Option<String> {
        let json: serde_json::Value = serde_json::from_str(body).ok()?;
        json.get("data")?
            .get("url")?
            .as_str()
            .map(str::to_string)
    }

    fn upload_to_imgbb(&self, image_bytes: &[u8]) -> Option<String> {
        use reqwest::blocking::multipart;

        if IMGBB_API_KEY.is_empty() {
            return None;
        }

        let form = multipart::Form::new().part(
            "image",
            multipart::Part::bytes(image_bytes.to_vec()).file_name("cover.jpg"),
        );
        let url = format!("https://api.imgbb.com/1/upload?key={IMGBB_API_KEY}");

        let response = self.client.post(&url).multipart(form).send().ok()?;

        if response.status().is_success() {
            let body = response.text().ok()?;
            return Self::parse_imgbb_response(&body);
        }

        None
    }

    fn upload_to_catbox(&self, image_bytes: &[u8], cover_path: &str) -> Option<String> {
        use reqwest::blocking::multipart;

        let extension = Path::new(cover_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        let file_name = format!("cover.{}", extension);

        let form = multipart::Form::new().text("reqtype", "fileupload").part(
            "fileToUpload",
            multipart::Part::bytes(image_bytes.to_vec()).file_name(file_name),
        );

        let response = self
            .client
            .post("https://catbox.moe/user/api.php")
            .multipart(form)
            .send()
            .ok()?;

        if response.status().is_success() {
            if let Ok(url) = response.text() {
                let trimmed = url.trim();
                if trimmed.starts_with("http") {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn upload_to_0x0(&self, image_bytes: &[u8], cover_path: &str) -> Option<String> {
        use reqwest::blocking::multipart;

        let extension = Path::new(cover_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        let file_name = format!("cover.{}", extension);

        let form = multipart::Form::new().part(
            "file",
            multipart::Part::bytes(image_bytes.to_vec()).file_name(file_name),
        );

        let response = self
            .client
            .post("https://0x0.st")
            .multipart(form)
            .send()
            .ok()?;

        if response.status().is_success() {
            if let Ok(url) = response.text() {
                let trimmed = url.trim();
                if trimmed.starts_with("http") {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn upload_to_envs(&self, image_bytes: &[u8], cover_path: &str) -> Option<String> {
        use reqwest::blocking::multipart;

        let extension = Path::new(cover_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");
        let file_name = format!("cover.{}", extension);

        let form = multipart::Form::new().part(
            "file",
            multipart::Part::bytes(image_bytes.to_vec()).file_name(file_name),
        );

        let response = self
            .client
            .post("https://envs.sh")
            .multipart(form)
            .send()
            .ok()?;

        if response.status().is_success() {
            if let Ok(url) = response.text() {
                let trimmed = url.trim();
                if trimmed.starts_with("http") {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn parse_release_mbid(json_body: &str) -> Option<String> {
        let metadata: MusicBrainzResponse = serde_json::from_str(json_body).ok()?;
        metadata
            .releases
            .into_iter()
            .find(|r| r.score.unwrap_or(0) > 50)
            .map(|r| r.id)
    }

    fn parse_cover_art_url(json_body: &str) -> Option<String> {
        let cover_data: CoverArtArchiveResponse = serde_json::from_str(json_body).ok()?;
        cover_data
            .images
            .into_iter()
            .find(|img| img.front && img.approved)
            .map(|img| img.thumbnails.large)
    }

    fn get_release_mbid(&self, song: &Song) -> Option<String> {
        let query = format!(
            "release:\"{}\" AND artist:\"{}\"",
            song.album, song.album_artist
        );
        let encoded_query =
            percent_encoding::utf8_percent_encode(&query, percent_encoding::NON_ALPHANUMERIC);
        let url =
            format!("https://musicbrainz.org/ws/2/release/?query={encoded_query}&fmt=json&limit=8");

        let response = self.client.get(&url).send().ok()?;

        if !response.status().is_success() {
            return None;
        }

        let body = response.text().ok()?;
        Self::parse_release_mbid(&body)
    }

    fn get_cover_art_url(&self, mbid: &str) -> Result<Option<String>, reqwest::Error> {
        let url = format!("https://coverartarchive.org/release/{mbid}");
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let body = match response.text() {
            Ok(b) => b,
            Err(e) => return Err(e),
        };

        Ok(Self::parse_cover_art_url(&body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_release_mbid_valid() {
        let json = r#"{
            "count": 2,
            "releases": [
                {
                    "id": "release-id-low-score",
                    "title": "Low Score Album",
                    "score": 40
                },
                {
                    "id": "release-id-high-score",
                    "title": "High Score Album",
                    "score": 95
                }
            ]
        }"#;

        assert_eq!(
            CoverFetcher::parse_release_mbid(json),
            Some("release-id-high-score".to_string())
        );
    }

    #[test]
    fn test_parse_release_mbid_no_high_score() {
        let json = r#"{
            "releases": [
                {
                    "id": "release-id-low",
                    "title": "Album",
                    "score": 50
                }
            ]
        }"#;

        assert_eq!(CoverFetcher::parse_release_mbid(json), None);
    }

    #[test]
    fn test_parse_release_mbid_empty_or_invalid() {
        assert_eq!(CoverFetcher::parse_release_mbid("{}"), None);
        assert_eq!(CoverFetcher::parse_release_mbid(r#"{"releases":[]}"#), None);
        assert_eq!(CoverFetcher::parse_release_mbid("not json"), None);
    }

    #[test]
    fn test_parse_cover_art_url_approved_front() {
        let json = r#"{
            "images": [
                {
                    "front": false,
                    "approved": true,
                    "thumbnails": { "large": "http://example.com/back.jpg" }
                },
                {
                    "front": true,
                    "approved": false,
                    "thumbnails": { "large": "http://example.com/unapproved.jpg" }
                },
                {
                    "front": true,
                    "approved": true,
                    "thumbnails": { "large": "http://example.com/front.jpg" }
                }
            ]
        }"#;

        assert_eq!(
            CoverFetcher::parse_cover_art_url(json),
            Some("http://example.com/front.jpg".to_string())
        );
    }

    #[test]
    fn test_parse_cover_art_url_no_matching_images() {
        let json = r#"{
            "images": [
                {
                    "front": true,
                    "approved": false,
                    "thumbnails": { "large": "http://example.com/unapproved.jpg" }
                }
            ]
        }"#;

        assert_eq!(CoverFetcher::parse_cover_art_url(json), None);
        assert_eq!(CoverFetcher::parse_cover_art_url("{}"), None);
        assert_eq!(CoverFetcher::parse_cover_art_url("invalid"), None);
    }

    #[test]
    fn test_parse_imgbb_response_valid() {
        let json = r#"{
            "data": {
                "id": "2ndCYJK",
                "title": "cover",
                "url_viewer": "https://ibb.co/2ndCYJK",
                "url": "https://i.ibb.co/w04MgRZ/cover.jpg",
                "display_url": "https://i.ibb.co/w04MgRZ/cover.jpg",
                "width": 500,
                "height": 500,
                "size": 42000,
                "time": 1552042650,
                "expiration": 0,
                "image": {
                    "filename": "cover.jpg",
                    "name": "cover",
                    "mime": "image/jpeg",
                    "extension": "jpg",
                    "url": "https://i.ibb.co/w04MgRZ/cover.jpg"
                }
            },
            "success": true,
            "status": 200
        }"#;

        assert_eq!(
            CoverFetcher::parse_imgbb_response(json),
            Some("https://i.ibb.co/w04MgRZ/cover.jpg".to_string())
        );
    }

    #[test]
    fn test_parse_imgbb_response_missing_or_invalid() {
        assert_eq!(CoverFetcher::parse_imgbb_response("{}"), None);
        assert_eq!(CoverFetcher::parse_imgbb_response(r#"{"data":{}}"#), None);
        assert_eq!(CoverFetcher::parse_imgbb_response("not json"), None);
        assert_eq!(CoverFetcher::parse_imgbb_response(r#"{"status":400,"error":{"message":"Bad request"}}"#), None);
    }
}
