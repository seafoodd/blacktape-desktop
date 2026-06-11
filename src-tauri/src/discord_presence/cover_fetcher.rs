use quick_xml::de::from_str;
use rand::RngExt;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{collections::HashMap, fs, sync::Mutex};

use crate::types::Song;

#[derive(Debug, Deserialize, Serialize)]
struct MusicBrainzMetadata {
    #[serde(rename = "release-list")]
    release_list: ReleaseList,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReleaseList {
    #[serde(rename = "release", default)]
    releases: Vec<Release>,

    #[serde(rename = "@count", default)]
    #[allow(dead_code)]
    count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Release {
    #[serde(rename = "@id")]
    id: String,

    #[serde(rename = "title")]
    title: String,

    #[serde(rename = "@score", default)]
    score: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub struct CoverArtArchiveResponse {
    pub images: Vec<CoverImage>,
}

#[derive(Debug, Deserialize)]
pub struct CoverImage {
    pub front: bool,
    pub approved: bool,
    pub image: String,
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

        // Local image upload to Catbox
        if let Some(local_art_bytes) = self.extract_local_artwork(song) {
            if let Some(ref local_path_str) = song.cover_url {
                if let Some(catbox_url) = self.upload_to_catbox(local_art_bytes, local_path_str) {
                    self.cache
                        .lock()
                        .unwrap()
                        .insert(song.path.clone(), catbox_url.clone());
                    return Some(catbox_url);
                }
            }
            println!("Catbox upload failed, falling back to MusicBrainz.");
        }

        // Fallback MusicBrainz image parse
        let mbid = self.get_release_mbid(song)?;

        match self.get_cover_art_url(&mbid) {
            Ok(Some(url)) => {
                // Save in cache
                self.cache
                    .lock()
                    .unwrap()
                    .insert(song.path.clone(), url.clone());
                Some(url)
            }
            Ok(None) => None,
            Err(e) => {
                eprintln!("Error fetching cover: {e}");
                None
            }
        }
    }

    fn extract_local_artwork(&self, song: &Song) -> Option<Vec<u8>> {
        if let Some(ref local_path_str) = song.cover_url {
            if !local_path_str.starts_with("http://") && !local_path_str.starts_with("https://") {
                let path = Path::new(local_path_str);
                if path.exists() && path.is_file() {
                    if let Ok(bytes) = fs::read(path) {
                        // println!("Loading pre-extracted cover art from database path: {}", local_path_str);
                        return Some(bytes);
                    }
                }
            }
        }

        None
    }

    fn upload_to_catbox(&self, mut image_bytes: Vec<u8>, cover_path: &str) -> Option<String> {
        use reqwest::blocking::multipart;

        if image_bytes.is_empty() || image_bytes.len() < 100 {
            eprintln!("Skipping upload: Image file data is corrupt or empty.");
            return None;
        }

        // append 4 random bytes to force catbox to create a fresh url
        let mut rng = rand::rng();
        let random_padding: [u8; 4] = rng.random();
        image_bytes.extend_from_slice(&random_padding);

        let extension = Path::new(cover_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("jpg");

        let file_name = format!("cover.{}", extension);

        let form = multipart::Form::new()
            .text("reqtype", "fileupload")
            .text("userhash", "") // Anonymous upload
            .part(
                "fileToUpload",
                multipart::Part::bytes(image_bytes).file_name(file_name), // Dynamic matching extension!
            );

        let response = self
            .client
            .post("https://catbox.moe/user/api.php")
            .multipart(form)
            .send()
            .ok()?;

        if response.status().is_success() {
            if let Ok(url_text) = response.text() {
                let trimmed = url_text.trim();
                if trimmed.starts_with("https://") {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    fn get_release_mbid(&self, song: &Song) -> Option<String> {
        let query = format!("release:\"{}\" AND artist:\"{}\"", song.album, song.artist);
        let encoded_query = urlencoding::encode(&query);

        let url =
            format!("https://musicbrainz.org/ws/2/release/?query={encoded_query}&fmt=xml&limit=8");

        // println!("Querying MusicBrainz: {}", url);

        let response = match self.client.get(&url).send() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("HTTP request failed: {e}");
                return None;
            }
        };

        // println!("Status: {}", response.status());

        if !response.status().is_success() {
            eprintln!("MusicBrainz API error: {}", response.status());
            return None;
        }

        let xml_body = match response.text() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to read response body: {e}");
                return None;
            }
        };

        let cleaned_xml = strip_xml_namespaces(&xml_body);

        let metadata: MusicBrainzMetadata = match from_str(&cleaned_xml) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to parse XML: {e}");
                eprintln!(
                    "Cleaned XML snippet:\n{}",
                    cleaned_xml.chars().take(300).collect::<String>()
                );
                return None;
            }
        };

        // println!("Parsed {} releases", metadata.release_list.releases.len());

        metadata
            .release_list
            .releases
            .into_iter()
            .find(|r| r.score.unwrap_or(0) > 50)
            .map(|r| {
                // println!(
                //     "Selected release: {} (score: {})",
                //     r.title,
                //     r.score.unwrap_or(0)
                // );
                r.id
            })
    }

    fn get_cover_art_url(&self, mbid: &str) -> Result<Option<String>, reqwest::Error> {
        let url = format!("https://coverartarchive.org/release/{mbid}");

        // println!("Querying Cover Art Archive: {}", url);

        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            // eprintln!("Cover Art Archive API error: {}", response.status());
            return Ok(None);
        }

        let cover_data: CoverArtArchiveResponse = response.json()?;

        Ok(cover_data
            .images
            .into_iter()
            .find(|img| img.front && img.approved)
            .map(|img| img.thumbnails.large))
    }
}

fn strip_xml_namespaces(xml: &str) -> String {
    xml.replace("xmlns=\"http://musicbrainz.org/ns/mmd-2.0#\"", "")
        .replace("xmlns:ns2=\"http://musicbrainz.org/ns/ext#-2.0\"", "")
        .replace("ns2:", "")
}
