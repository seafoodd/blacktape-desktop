use crate::metadata::types::{
    CanonicalDisc, CanonicalRelease, CanonicalReleaseSummary, CanonicalTrack, ReleaseEditionOption,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

const MUSICBRAINZ_USER_AGENT: &str = "Blacktape/0.8.0 ( mail.seafood@proton.me; https://github.com/seafoodd/blacktape-desktop )";
const RATE_LIMIT_INTERVAL_MS: u64 = 1150;
const MB_RETRY_BUDGET: Duration = Duration::from_millis(6500);
const MB_MAX_ATTEMPTS: usize = 3;
const SUPERSEDED: &str = "This MusicBrainz search was replaced by a newer one.";

static GLOBAL_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static GLOBAL_REQUEST_GATE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn shared_http_client() -> &'static Client {
    GLOBAL_HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(MUSICBRAINZ_USER_AGENT)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    })
}

fn shared_rate_limiter() -> &'static Mutex<Option<Instant>> {
    GLOBAL_REQUEST_GATE.get_or_init(|| Mutex::new(None))
}

#[derive(Clone)]
pub struct MusicBrainzClient {
    cache_dir: Option<PathBuf>,
    wanted: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl MusicBrainzClient {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self { cache_dir, wanted: None }
    }

    /// Requests that are no longer wanted when their turn comes are not sent, so they do not delay newer ones.
    pub fn only_while(mut self, wanted: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.wanted = Some(Arc::new(wanted));
        self
    }

    fn ensure_wanted(&self) -> Result<(), String> {
        match &self.wanted {
            Some(wanted) if !wanted() => Err(SUPERSEDED.into()),
            _ => Ok(()),
        }
    }

    async fn get_json_with_retry(&self, url: &str, context: &str) -> Result<String, String> {
        let started = Instant::now();
        let mut last_error = String::new();
        let mut extra_wait: Option<Duration> = None;

        for attempt in 0..MB_MAX_ATTEMPTS {
            if attempt > 0 && started.elapsed() >= MB_RETRY_BUDGET {
                break;
            }
            if let Some(delay) = extra_wait.take() {
                tokio::time::sleep(delay).await;
            }

            // Hold the gate across spacing + HTTP send so two requests never hit MusicBrainz
            // concurrently and the 1.15s cooldown is measured from response completion.
            let resp_result = {
                let mut last_time = shared_rate_limiter().lock().await;
                self.ensure_wanted()?;
                if let Some(prev) = *last_time {
                    let elapsed = prev.elapsed();
                    if elapsed < Duration::from_millis(RATE_LIMIT_INTERVAL_MS) {
                        let delay = Duration::from_millis(RATE_LIMIT_INTERVAL_MS) - elapsed;
                        tokio::time::sleep(delay).await;
                    }
                }
                // A newer search may have arrived during the spacing delay.
                self.ensure_wanted()?;
                let res = shared_http_client().get(url).send().await;
                *last_time = Some(Instant::now());
                res
            };

            match resp_result {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return resp
                            .text()
                            .await
                            .map_err(|e| format!("Failed to read MusicBrainz response body: {e}"));
                    }

                    let retryable = status == StatusCode::SERVICE_UNAVAILABLE
                        || status == StatusCode::TOO_MANY_REQUESTS
                        || status.is_server_error();
                    let retry_after_ms = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|secs| secs.saturating_mul(1000));

                    last_error = if status == StatusCode::SERVICE_UNAVAILABLE {
                        "MusicBrainz is temporarily busy (503). Please try again.".to_string()
                    } else {
                        format!("MusicBrainz {context} returned status {status}")
                    };

                    if !retryable || attempt + 1 >= MB_MAX_ATTEMPTS {
                        break;
                    }

                    // The gate already enforces RATE_LIMIT_INTERVAL_MS before the next attempt;
                    // only sleep any extra backoff beyond that interval.
                    let target_ms = retry_after_ms.unwrap_or(1400 * (attempt as u64 + 1));
                    let extra_ms = target_ms.saturating_sub(RATE_LIMIT_INTERVAL_MS);
                    let remaining = MB_RETRY_BUDGET.saturating_sub(started.elapsed());
                    let wait = Duration::from_millis(extra_ms).min(remaining);
                    if remaining < Duration::from_millis(RATE_LIMIT_INTERVAL_MS) {
                        break;
                    }
                    tracing::warn!(
                        attempt = attempt + 1,
                        status = %status,
                        extra_wait_ms = wait.as_millis(),
                        "MusicBrainz transient failure, retrying"
                    );
                    if !wait.is_zero() {
                        extra_wait = Some(wait);
                    }
                }
                Err(e) => {
                    let retryable = e.is_timeout() || e.is_connect();
                    last_error = format!("MusicBrainz {context} HTTP request failed: {e}");
                    if !retryable || attempt + 1 >= MB_MAX_ATTEMPTS {
                        break;
                    }
                    let remaining = MB_RETRY_BUDGET.saturating_sub(started.elapsed());
                    if remaining < Duration::from_millis(RATE_LIMIT_INTERVAL_MS) {
                        break;
                    }
                    tracing::warn!(
                        attempt = attempt + 1,
                        error = %e,
                        "MusicBrainz request failed, retrying"
                    );
                }
            }
        }

        Err(last_error)
    }

    fn cache_path(&self, key: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?.join("musicbrainz");
        let hash = xxhash_rust::xxh3::xxh3_64(key.as_bytes());
        Some(dir.join(format!("{hash:016x}.json")))
    }

    pub async fn read_cache(&self, key: &str) -> Option<String> {
        let path = self.cache_path(key)?;
        if !path.is_file() {
            return None;
        }
        // Cache valid for 7 days
        if let Ok(metadata) = std::fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed > Duration::from_secs(7 * 24 * 3600) {
                        return None;
                    }
                }
            }
        }
        tokio::fs::read_to_string(path).await.ok()
    }

    fn release_group_cache_key(lucene_query: &str) -> String {
        format!("rg_search_v2:{}", lucene_query.trim().to_lowercase())
    }

    /// The saved answer to this search however old it is, for when MusicBrainz cannot answer now.
    pub async fn saved_release_groups(&self, query: &str) -> Option<Vec<MbReleaseGroupSummary>> {
        let key = Self::release_group_cache_key(&build_mb_lucene_query(query.trim()));
        let saved = tokio::fs::read_to_string(self.cache_path(&key)?).await.ok()?;
        serde_json::from_str::<MbReleaseGroupSearchResponse>(&saved).ok().map(|parsed| parsed.release_groups)
    }

    pub async fn write_cache(&self, key: &str, content: &str) {
        if let Some(path) = self.cache_path(key) {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::write(path, content).await;
        }
    }

    async fn search_release_groups_lucene(
        &self,
        lucene_query: &str,
    ) -> Result<Vec<MbReleaseGroupSummary>, String> {
        let trimmed = lucene_query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let cache_key = Self::release_group_cache_key(trimmed);
        if let Some(cached) = self.read_cache(&cache_key).await {
            if let Ok(parsed) = serde_json::from_str::<MbReleaseGroupSearchResponse>(&cached) {
                tracing::debug!("Cache hit for MusicBrainz release-group search");
                return Ok(parsed.release_groups);
            }
        }

        let encoded = urlencoding_simple(trimmed);
        let url =
            format!("https://musicbrainz.org/ws/2/release-group/?query={encoded}&fmt=json&limit=15");

        tracing::info!(url = %url, "Fetching release-groups from MusicBrainz");
        let body = self
            .get_json_with_retry(&url, "release-group search")
            .await?;

        let parsed: MbReleaseGroupSearchResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse MusicBrainz release-group JSON: {e}"))?;

        self.write_cache(&cache_key, &body).await;
        Ok(parsed.release_groups)
    }

    /// Searches MusicBrainz for canonical release-groups matching `query` (avoids 15+ regional
    /// pressing duplicates crowding out distinct albums).
    #[tracing::instrument(skip(self), fields(query = %query))]
    pub async fn search_release_groups(
        &self,
        query: &str,
    ) -> Result<Vec<MbReleaseGroupSummary>, String> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let lucene_query = build_mb_lucene_query(trimmed);
        self.search_release_groups_lucene(&lucene_query).await
    }

    async fn resolve_release_id_for_group(&self, rg_id: &str) -> Result<String, String> {
        let map_key = format!("rg_release_v7:{rg_id}");
        if let Some(cached_id) = self.read_cache(&map_key).await {
            let trimmed = cached_id.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }

        if let Ok(editions) = self.list_grouped_editions_for_group(rg_id).await {
            if let Some(rel_id) = editions
                .iter()
                .find(|e| e.is_default)
                .or_else(|| editions.first())
                .map(|e| e.id.clone())
            {
                self.write_cache(&map_key, &rel_id).await;
                return Ok(rel_id);
            }
        }

        let url =
            format!("https://musicbrainz.org/ws/2/release-group/{rg_id}?inc=releases&fmt=json");
        tracing::info!(url = %url, "Resolving preferred release for MusicBrainz release-group");
        let body = self
            .get_json_with_retry(&url, "release-group lookup")
            .await?;
        let details: MbReleaseGroupDetails = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse release-group details: {e}"))?;

        let rel_id = pick_preferred_release_id(&details.title, &details.releases).ok_or_else(
            || format!("Release-group '{rg_id}' has no releases on MusicBrainz"),
        )?;
        self.write_cache(&map_key, &rel_id).await;
        Ok(rel_id)
    }

    /// Fetches the full canonical release with complete tracklist, disc structures, and cover art.
    /// Accepts either a direct release MBID or `rg:<release-group-mbid>`.
    #[tracing::instrument(skip(self), fields(mbid = %mbid))]
    pub async fn fetch_canonical_release(&self, mbid: &str) -> Result<CanonicalRelease, String> {
        let cache_key = format!("release_v7:{}", mbid);
        if let Some(cached) = self.read_cache(&cache_key).await {
            if let Ok(parsed) = serde_json::from_str::<CanonicalRelease>(&cached) {
                tracing::debug!("Cache hit for MusicBrainz canonical release");
                return Ok(parsed);
            }
        }
        if let Some(rg_id) = mbid.strip_prefix("rg:") {
            if let Some(rel_id) = self.read_cache(&format!("rg_release_v7:{rg_id}")).await {
                let rel_id = rel_id.trim();
                if !rel_id.is_empty() {
                    if let Some(cached) = self.read_cache(&format!("release_v7:{rel_id}")).await {
                        if let Ok(mut parsed) = serde_json::from_str::<CanonicalRelease>(&cached) {
                            parsed.id = mbid.to_string();
                            if parsed.release_group_id.is_none() {
                                parsed.release_group_id = Some(rg_id.to_string());
                            }
                            return Ok(parsed);
                        }
                    }
                }
            }
        }

        if mbid.starts_with("synth:") || mbid.starts_with("http://") || mbid.starts_with("https://") {
            return Err(format!("Direct/synthesized URL '{mbid}' not found in cache"));
        }

        let (release_mbid, rg_cover_hint, canonical_id) = if let Some(rg_id) = mbid.strip_prefix("rg:") {
            let rel_id = self.resolve_release_id_for_group(rg_id).await?;
            let rg_cover = format!("https://coverartarchive.org/release-group/{rg_id}/front-500");
            (rel_id, Some(rg_cover), mbid.to_string())
        } else {
            (mbid.to_string(), None, mbid.to_string())
        };

        let url = release_lookup_url(&release_mbid);
        tracing::info!(url = %url, "Fetching release details from MusicBrainz");

        let body = self
            .get_json_with_retry(&url, "release")
            .await?;

        let raw: MbReleaseDetails = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse MusicBrainz release details: {e}"))?;

        let fallback_rg = mbid.strip_prefix("rg:").map(str::to_string);
        let derived_cover = rg_cover_hint.or_else(|| {
            raw.release_group
                .as_ref()
                .map(|g| format!("https://coverartarchive.org/release-group/{}/front-500", g.id))
                .or_else(|| Some(format!("https://coverartarchive.org/release/{release_mbid}/front-500")))
        });
        let mut release = parse_mb_release_details(raw, derived_cover);
        if release.release_group_id.is_none() {
            release.release_group_id = fallback_rg;
        }
        release.id = canonical_id;
        if let Ok(serialized) = serde_json::to_string(&release) {
            self.write_cache(&cache_key, &serialized).await;
        }

        Ok(release)
    }

    /// Fetches all releases in a MusicBrainz release-group, deduplicates regional/reissue clones,
    /// and returns distinct album editions suitable for a release picker dropdown.
    #[tracing::instrument(skip(self), fields(rg_id = %rg_id))]
    pub async fn list_grouped_editions_for_group(
        &self,
        rg_id: &str,
    ) -> Result<Vec<ReleaseEditionOption>, String> {
        let clean_rg = rg_id.trim().trim_start_matches("canonical:").trim_start_matches("rg:");
        if clean_rg.is_empty() {
            return Ok(Vec::new());
        }

        let cache_key = format!("rg_editions_v4:{clean_rg}");
        if let Some(cached) = self.read_cache(&cache_key).await {
            if let Ok(parsed) = serde_json::from_str::<Vec<ReleaseEditionOption>>(&cached) {
                return Ok(parsed);
            }
        }

        let url = format!(
            "https://musicbrainz.org/ws/2/release?release-group={clean_rg}&inc=media&limit=100&fmt=json"
        );
        let body = self.get_json_with_retry(&url, "release list").await?;
        let parsed: MbReleaseListResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Failed to parse MusicBrainz release list JSON: {e}"))?;

        // Infer canonical title from earliest Official release (or first item)
        let rg_title = parsed
            .releases
            .iter()
            .find(|r| r.status.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("Official")))
            .or_else(|| parsed.releases.first())
            .map(|r| r.title.clone())
            .unwrap_or_default();

        let grouped = group_releases_for_picker(&rg_title, &parsed.releases);
        if let Some(default_opt) = grouped.iter().find(|g| g.is_default) {
            self.write_cache(&format!("rg_release_v7:{clean_rg}"), &default_opt.id)
                .await;
        }
        if let Ok(serialized) = serde_json::to_string(&grouped) {
            self.write_cache(&cache_key, &serialized).await;
        }
        Ok(grouped)
    }

    /// Returns a cached MusicBrainz canonical release for `(artist, album_title)` without making network calls.
    pub async fn try_cached_canonical_for_album(
        &self,
        artist: &str,
        album_title: &str,
    ) -> Option<CanonicalRelease> {
        let norm_artist = crate::search::ranking::normalize(artist);
        let norm_title = crate::search::ranking::normalize(album_title);
        if norm_artist.is_empty() || norm_title.is_empty() {
            return None;
        }
        let match_key = format!("rg_match_v1:{norm_artist}::{norm_title}");
        let rg_id = if let Some(cached_rg) = self.read_cache(&match_key).await {
            let trimmed = cached_rg.trim().to_string();
            if trimmed.is_empty() {
                return None;
            }
            trimmed
        } else {
            let query = format!("{} {}", artist.trim(), album_title.trim());
            let lucene_query = build_mb_lucene_query(&query);
            let search_key = format!("rg_search_v2:{}", lucene_query.to_lowercase());
            let cached = self.read_cache(&search_key).await?;
            let parsed = serde_json::from_str::<MbReleaseGroupSearchResponse>(&cached).ok()?;
            pick_matching_release_group(artist, album_title, &parsed.release_groups)?
        };

        let rel_id = self
            .read_cache(&format!("rg_release_v7:{rg_id}"))
            .await
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(ref rid) = rel_id {
            if let Some(cached_rel) = self.read_cache(&format!("release_v7:{rid}")).await {
                if let Ok(mut parsed) = serde_json::from_str::<CanonicalRelease>(&cached_rel) {
                    parsed.id = format!("rg:{rg_id}");
                    if parsed.release_group_id.is_none() {
                        parsed.release_group_id = Some(rg_id.clone());
                    }
                    return Some(parsed);
                }
            }
        }
        if let Some(cached_rg_rel) = self.read_cache(&format!("release_v7:rg:{rg_id}")).await {
            if let Ok(parsed) = serde_json::from_str::<CanonicalRelease>(&cached_rg_rel) {
                return Some(parsed);
            }
        }
        None
    }

    /// Attempts to find a matching MusicBrainz release-group for a platform album `(artist, title)`.
    pub async fn find_matching_release_group_id(
        &self,
        artist: &str,
        album_title: &str,
    ) -> Option<String> {
        let norm_artist = crate::search::ranking::normalize(artist);
        let norm_title = crate::search::ranking::normalize(album_title);
        if norm_artist.is_empty() || norm_title.is_empty() {
            return None;
        }
        let match_key = format!("rg_match_v1:{norm_artist}::{norm_title}");
        if let Some(cached_rg) = self.read_cache(&match_key).await {
            let trimmed = cached_rg.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        let query = format!("{} {}", artist.trim(), album_title.trim());
        if let Ok(Ok(groups)) = tokio::time::timeout(
            Duration::from_millis(6500),
            self.search_release_groups(&query),
        )
        .await
        {
            if let Some(matched) = pick_matching_release_group(artist, album_title, &groups) {
                self.write_cache(&match_key, &matched).await;
                return Some(matched);
            }
        }

        let clean_artist = sanitize_lucene_phrase(artist);
        let clean_title = sanitize_lucene_phrase(album_title);
        if !clean_artist.is_empty() && !clean_title.is_empty() {
            let fielded = format!(r#"artist:"{clean_artist}" AND releasegroup:"{clean_title}""#);
            if let Ok(Ok(groups)) = tokio::time::timeout(
                Duration::from_millis(6500),
                self.search_release_groups_lucene(&fielded),
            )
            .await
            {
                if let Some(matched) = pick_matching_release_group(artist, album_title, &groups) {
                    self.write_cache(&match_key, &matched).await;
                    return Some(matched);
                }
            }
        }

        None
    }
}

fn release_lookup_url(release_mbid: &str) -> String {
    format!(
        "https://musicbrainz.org/ws/2/release/{release_mbid}?inc=recordings+artist-credits+media+release-groups&fmt=json"
    )
}

fn urlencoding_simple(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn format_rank_from_media(media: &[MbReleaseStubMedium]) -> u8 {
    if media.is_empty() {
        return 2;
    }
    let has_cd = media.iter().any(|m| {
        m.format.as_deref().is_some_and(|f| {
            let l = f.to_ascii_lowercase();
            l.contains("cd") && !l.contains("sacd") && !l.contains("dualdisc")
        })
    });
    if has_cd {
        return 0;
    }
    let has_digital = media
        .iter()
        .any(|m| m.format.as_deref().is_some_and(|f| f.to_ascii_lowercase().contains("digital")));
    if has_digital {
        return 1;
    }
    let has_sacd = is_sacd_or_dualdisc_media(media);
    if has_sacd {
        return 4;
    }
    let has_vinyl = media
        .iter()
        .any(|m| m.format.as_deref().is_some_and(|f| f.to_ascii_lowercase().contains("vinyl")));
    if has_vinyl {
        return 3;
    }
    let has_cassette = media.iter().any(|m| {
        m.format
            .as_deref()
            .is_some_and(|f| f.to_ascii_lowercase().contains("cassette"))
    });
    if has_cassette {
        return 5;
    }
    2
}

fn normalize_date_sort_key(date: Option<&str>) -> String {
    let raw = date.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        return "9999-99-99".to_string();
    }
    let mut parts = raw.split('-').filter(|p| !p.is_empty());
    let Some(y) = parts.next() else {
        return "9999-99-99".to_string();
    };
    let m = parts.next().unwrap_or("99");
    let d = parts.next().unwrap_or("99");
    format!("{y:0>4}-{m:0>2}-{d:0>2}")
}

fn is_vinyl_or_cassette_media(media: &[MbReleaseStubMedium]) -> bool {
    !media.is_empty()
        && media.iter().all(|m| {
            m.format.as_deref().is_some_and(|f| {
                let l = f.to_ascii_lowercase();
                l.contains("vinyl") || l.contains("cassette")
            })
        })
}

fn is_sacd_or_dualdisc_media(media: &[MbReleaseStubMedium]) -> bool {
    !media.is_empty()
        && media.iter().any(|m| {
            m.format.as_deref().is_some_and(|f| {
                let l = f.to_ascii_lowercase();
                l.contains("sacd") || l.contains("dualdisc")
            })
        })
}

fn has_multi_disc_cd_media(media: &[MbReleaseStubMedium]) -> bool {
    media.len() >= 2
        && !is_sacd_or_dualdisc_media(media)
        && media.iter().any(|m| {
            m.format
                .as_deref()
                .is_some_and(|f| f.to_ascii_lowercase().contains("cd"))
        })
}

fn is_variant_or_deluxe_release(clean_rg_title: &str, r: &MbReleaseStub) -> bool {
    if is_sacd_or_dualdisc_media(&r.media) {
        return true;
    }
    if !extract_variant_flags(&r.title, r.disambiguation.as_deref().unwrap_or(""), clean_rg_title)
        .is_empty()
    {
        return true;
    }
    let combined = format!(
        "{} {}",
        r.title,
        r.disambiguation.as_deref().unwrap_or("")
    )
    .to_ascii_lowercase();
    const DELUXE_WORDS: &[&str] = &[
        "deluxe", "expanded", "anniversary", "super deluxe", "collector", "box set",
        "dolby atmos", "atmos", "surround", "5.1", "remaster", "remastered", "hdcd", "shm",
    ];
    let clean_rg_lower = clean_rg_title.to_ascii_lowercase();
    DELUXE_WORDS
        .iter()
        .any(|w| combined.contains(w) && !clean_rg_lower.contains(w))
}

fn release_preference_key(
    clean_title: &str,
    r: &MbReleaseStub,
) -> (u8, bool, bool, i32, bool, String, u8, usize, String) {
    let is_official = r
        .status
        .as_deref()
        .map_or(false, |s| s.eq_ignore_ascii_case("Official"));
    let is_bootleg = r
        .status
        .as_deref()
        .map_or(false, |s| s.eq_ignore_ascii_case("Bootleg"));
    let title_exact = r.title.trim().eq_ignore_ascii_case(clean_title);
    let tier = if is_official && title_exact {
        0
    } else if is_official {
        1
    } else if !is_bootleg {
        2
    } else {
        3
    };
    let is_variant_or_deluxe = is_variant_or_deluxe_release(clean_title, r);
    let is_vinyl_or_cass = is_vinyl_or_cassette_media(&r.media);
    let year = parse_year_from_date(r.date.as_deref()).unwrap_or(9999);
    let not_multi_cd = !has_multi_disc_cd_media(&r.media);
    let date_key = normalize_date_sort_key(r.date.as_deref());
    let fmt_rank = format_rank_from_media(&r.media);
    let disc_count = if r.media.is_empty() { 1 } else { r.media.len() };
    (
        tier,
        is_variant_or_deluxe,
        is_vinyl_or_cass,
        year,
        not_multi_cd,
        date_key,
        fmt_rank,
        disc_count,
        r.id.clone(),
    )
}

pub fn pick_preferred_release_id(rg_title: &str, releases: &[MbReleaseStub]) -> Option<String> {
    let clean_title = rg_title.trim();
    releases
        .iter()
        .filter(|r| !r.id.is_empty())
        .min_by_key(|r| release_preference_key(clean_title, r))
        .map(|r| r.id.clone())
}

pub fn is_bootleg_only_group(releases: &[MbReleaseStub]) -> bool {
    !releases.is_empty()
        && releases.iter().all(|r| {
            r.status
                .as_deref()
                .map_or(false, |s| s.eq_ignore_ascii_case("Bootleg"))
        })
}

fn parse_year_from_date(date: Option<&str>) -> Option<i32> {
    date.map(str::trim)
        .filter(|d| !d.is_empty())
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse::<i32>().ok())
}

fn strip_trailing_bracketed_edition_noise(title: &str) -> String {
    const EDITION_BRACKET_WORDS: &[&str] = &[
        "remaster", "remastered", "reissue", "anniversary", "deluxe", "expanded", "edition",
        "version", "explicit", "clean", "edited", "censored", "bonus", "special", "limited",
        "vinyl", "cd", "digital", "download", "stereo", "mono", "instrumental", "instrumentals",
        "acapella", "acappella", "acoustic", "unplugged", "demo", "demos", "live", "remix",
        "remixes", "commentary", "karaoke", "disc", "track", "tracks", "uk", "us", "japan",
        "international", "standard", "collector", "collectors", "platinum", "super", "import",
    ];
    let mut current = title.trim().to_string();
    loop {
        let trimmed = current.trim_end();
        let close = match trimmed.chars().last() {
            Some(')') => '(',
            Some(']') => '[',
            _ => break,
        };
        let Some(open_idx) = trimmed.rfind(close) else {
            break;
        };
        let inner = &trimmed[open_idx + 1..trimmed.len() - 1];
        let norm_inner = crate::search::ranking::normalize(inner);
        let words: Vec<&str> = norm_inner.split_whitespace().collect();
        let is_edition_bracket = !words.is_empty()
            && words.iter().all(|w| {
                w.chars().all(|c| c.is_ascii_digit())
                    || EDITION_BRACKET_WORDS.contains(w)
                    || w.ends_with("th")
                    || w.ends_with("st")
                    || w.ends_with("nd")
                    || w.ends_with("rd")
            });
        if is_edition_bracket {
            current = trimmed[..open_idx].trim_end().to_string();
        } else {
            break;
        }
    }
    crate::search::ranking::normalize(&current)
}

fn extract_variant_flags(title: &str, disambiguation: &str, rg_title: &str) -> Vec<&'static str> {
    let rg_norm = crate::search::ranking::normalize(rg_title);
    let rg_words: Vec<&str> = rg_norm.split_whitespace().collect();

    let combined = format!("{} {}", title, disambiguation);
    let norm = crate::search::ranking::normalize(&combined);
    let words: Vec<&str> = norm.split_whitespace().collect();

    let mut flags = BTreeSet::new();
    for w in &words {
        match *w {
            "clean" | "edited" | "censored" if !rg_words.contains(w) => {
                flags.insert("Clean");
            }
            "mono" if !rg_words.contains(w) => {
                flags.insert("Mono");
            }
            "instrumental" | "instrumentals" if !rg_words.contains(&"instrumental") && !rg_words.contains(&"instrumentals") => {
                flags.insert("Instrumental");
            }
            "acapella" | "acappella" if !rg_words.contains(w) => {
                flags.insert("Acapella");
            }
            "acoustic" | "unplugged" if !rg_words.contains(&"acoustic") && !rg_words.contains(&"unplugged") => {
                flags.insert("Acoustic");
            }
            "demo" | "demos" if !rg_words.contains(&"demo") && !rg_words.contains(&"demos") => {
                flags.insert("Demo");
            }
            "live" if !rg_words.contains(&"live") => {
                flags.insert("Live");
            }
            "remix" | "remixes" | "remixed"
                if !rg_words.contains(&"remix")
                    && !rg_words.contains(&"remixes")
                    && !rg_words.contains(&"remixed") =>
            {
                flags.insert("Remix");
            }
            "commentary" if !rg_words.contains(&"commentary") => {
                flags.insert("Commentary");
            }
            "karaoke" if !rg_words.contains(&"karaoke") => {
                flags.insert("Karaoke");
            }
            "chopped" | "screwed" if !rg_words.contains(&"chopped") && !rg_words.contains(&"screwed") => {
                flags.insert("Chopped & Screwed");
            }
            _ => {}
        }
    }
    let disambig_norm = crate::search::ranking::normalize(disambiguation);
    if disambig_norm.split_whitespace().any(|w| w == "mix") && !rg_words.contains(&"mix") {
        flags.insert("Alt Mix");
    }
    flags.into_iter().collect()
}

fn is_pressing_or_reissue_noise_phrase(raw: &str) -> bool {
    const NOISE_WORDS: &[&str] = &[
        "reissue", "remaster", "remastered", "anniversary", "limited", "edition", "numbered",
        "pressing", "rti", "180g", "180", "gram", "vinyl", "cd", "cassette", "digital", "media",
        "gatefold", "digipak", "jewel", "case", "cardboard", "sleeve", "slipcase", "box", "set",
        "shm", "hqcd", "sacd", "uhqcd", "hdcd", "encoded", "blu", "spec", "promo", "promotional", "advance",
        "white", "label", "test", "club", "bmg", "columbia", "house", "import", "export",
        "colored", "coloured", "clear", "black", "red", "blue", "green", "gold", "silver",
        "picture", "disc", "uk", "us", "eu", "europe", "japan", "japanese", "australian",
        "explicit", "stereo", "original", "release", "track", "tracks", "remasters",
    ];
    let norm = crate::search::ranking::normalize(raw);
    let words: Vec<&str> = norm.split_whitespace().collect();
    !words.is_empty()
        && words.iter().all(|w| {
            w.chars().all(|c| c.is_ascii_digit())
                || NOISE_WORDS.contains(w)
                || w.ends_with("th")
                || w.ends_with("st")
                || w.ends_with("nd")
                || w.ends_with("rd")
        })
}

fn summarize_media_format(media: &[MbReleaseStubMedium]) -> Option<String> {
    if media.is_empty() {
        return None;
    }
    let count = media.len();
    let first = media.iter().find_map(|m| m.format.as_deref())?.trim();
    if first.is_empty() {
        return None;
    }
    if count > 1 && !first.contains('×') && !first.to_ascii_lowercase().starts_with(&format!("{count}x")) {
        Some(format!("{count}×{first}"))
    } else {
        Some(first.to_string())
    }
}

fn extract_title_edition_suffix(release_title: &str, rg_title: &str) -> Option<String> {
    let rt = release_title.trim();
    let gt = rg_title.trim();
    if rt.len() > gt.len() && rt[..gt.len()].eq_ignore_ascii_case(gt) {
        let rest = rt[gt.len()..].trim();
        let inner = rest
            .trim_start_matches(&['(', '[', '-', ':'][..])
            .trim_end_matches(&[')', ']'][..])
            .trim();
        if !inner.is_empty() && !is_pressing_or_reissue_noise_phrase(inner) {
            return Some(inner.to_string());
        }
    }
    None
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn title_matches_rg_core(release_title: &str, rg_title: &str) -> bool {
    let rg_core = strip_trailing_bracketed_edition_noise(rg_title);
    let rel_core = strip_trailing_bracketed_edition_noise(release_title);
    if rg_core.is_empty() || rel_core.is_empty() {
        return true;
    }
    rel_core == rg_core || rel_core.starts_with(&rg_core)
}

fn pick_best_edition_label(
    rep: &MbReleaseStub,
    members: &[&MbReleaseStub],
    rg_title: &str,
    variant_flags: &[&'static str],
    track_count: usize,
    is_default: bool,
    title_mismatched: bool,
) -> String {
    if !variant_flags.is_empty() {
        let mut extras: Vec<String> = variant_flags.iter().map(|s| (*s).to_string()).collect();
        if let Some(d) = rep
            .disambiguation
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty() && d.to_ascii_lowercase().contains("mix"))
            .or_else(|| {
                members
                    .iter()
                    .filter_map(|r| r.disambiguation.as_deref())
                    .map(str::trim)
                    .find(|d| !d.is_empty() && d.to_ascii_lowercase().contains("mix"))
            })
        {
            return capitalize_first(d);
        }
        return extras.remove(0)
            + &extras
                .into_iter()
                .map(|x| format!(" · {x}"))
                .collect::<String>();
    }

    if is_default && !title_mismatched && extract_title_edition_suffix(&rep.title, rg_title).is_none() {
        return "Original edition".to_string();
    }

    if title_mismatched && !rep.title.trim().is_empty() {
        return rep.title.trim().to_string();
    }

    if let Some(suffix) = extract_title_edition_suffix(&rep.title, rg_title) {
        return capitalize_first(&suffix);
    }
    for r in members {
        if let Some(suffix) = extract_title_edition_suffix(&r.title, rg_title) {
            return capitalize_first(&suffix);
        }
    }

    // Prefer the representative release's own disambiguation first so another pressing's
    // quirk (e.g. a later "HDCD encoded" reissue) is never stamped onto an earlier pressing.
    if let Some(rep_d) = rep
        .disambiguation
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && !is_pressing_or_reissue_noise_phrase(d))
    {
        return capitalize_first(rep_d);
    }

    let clean_disambig = members
        .iter()
        .filter_map(|r| r.disambiguation.as_deref())
        .map(str::trim)
        .filter(|d| !d.is_empty() && !is_pressing_or_reissue_noise_phrase(d))
        // Prefer semantic edition names ("expanded edition", "deluxe") over bare count tags ("35 track reissue")
        .min_by_key(|d| {
            let l = d.to_ascii_lowercase();
            let priority = if l.contains("deluxe") || l.contains("expanded") {
                0
            } else if l.contains("reissue") && l.contains("track") {
                2
            } else {
                1
            };
            (priority, d.len())
        });
    if let Some(label) = clean_disambig {
        return capitalize_first(label);
    }

    if is_default {
        "Original edition".to_string()
    } else if track_count > 0 {
        format!("{track_count}-track edition")
    } else {
        "Standard edition".to_string()
    }
}

/// Groups releases from a MusicBrainz release-group into genuinely distinct album versions
/// (collapsing regional, physical-format, and reissue duplicates that share the same title,
/// content variants, and track count). Pure and deterministic.
pub fn group_releases_for_picker(
    rg_title: &str,
    releases: &[MbReleaseStub],
) -> Vec<ReleaseEditionOption> {
    let valid_base: Vec<&MbReleaseStub> = releases
        .iter()
        .filter(|r| !r.id.trim().is_empty())
        .collect();
    if valid_base.is_empty() {
        return Vec::new();
    }

    let has_official = valid_base
        .iter()
        .any(|r| r.status.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("Official")));
    let after_status: Vec<&MbReleaseStub> = if has_official {
        valid_base
            .into_iter()
            .filter(|r| {
                !r.status.as_deref().is_some_and(|s| {
                    s.eq_ignore_ascii_case("Bootleg") || s.eq_ignore_ascii_case("Pseudo-Release")
                })
            })
            .collect()
    } else {
        valid_base
    };

    let any_with_tracks = after_status
        .iter()
        .any(|r| r.media.iter().map(|m| m.track_count).sum::<usize>() > 0);
    let eligible: Vec<&MbReleaseStub> = if any_with_tracks {
        after_status
            .into_iter()
            .filter(|r| r.media.iter().map(|m| m.track_count).sum::<usize>() > 0)
            .collect()
    } else {
        after_status
    };
    if eligible.is_empty() {
        return Vec::new();
    }

    let eligible_owned: Vec<MbReleaseStub> = eligible.iter().map(|r| (*r).clone()).collect();
    let default_id = pick_preferred_release_id(rg_title, &eligible_owned);

    // Key: (core_title, variant_flags, total_tracks)
    let mut buckets: BTreeMap<(String, Vec<&'static str>, usize), Vec<&MbReleaseStub>> =
        BTreeMap::new();
    for r in eligible {
        let total_tracks: usize = r.media.iter().map(|m| m.track_count).sum();
        let title_core = strip_trailing_bracketed_edition_noise(&r.title);
        let variants = extract_variant_flags(
            &r.title,
            r.disambiguation.as_deref().unwrap_or(""),
            rg_title,
        );
        buckets
            .entry((title_core, variants, total_tracks))
            .or_default()
            .push(r);
    }

    let clean_rg_title = rg_title.trim();
    let mut out: Vec<ReleaseEditionOption> = Vec::with_capacity(buckets.len());

    for ((_, variants, total_tracks), mut members) in buckets {
        members.sort_by_key(|r| release_preference_key(clean_rg_title, r));
        let has_default = default_id
            .as_deref()
            .is_some_and(|def| members.iter().any(|r| r.id == def));
        let rep = if has_default {
            members
                .iter()
                .copied()
                .find(|r| Some(r.id.as_str()) == default_id.as_deref())
                .unwrap_or(members[0])
        } else {
            members[0]
        };
        let disc_count = if rep.media.is_empty() { 1 } else { rep.media.len() };
        let title_mismatched =
            !clean_rg_title.is_empty() && !title_matches_rg_core(&rep.title, clean_rg_title);
        let edition_label = pick_best_edition_label(
            rep,
            &members,
            clean_rg_title,
            &variants,
            total_tracks,
            has_default,
            title_mismatched,
        );
        let year = members
            .iter()
            .filter_map(|r| parse_year_from_date(r.date.as_deref()))
            .min()
            .or_else(|| parse_year_from_date(rep.date.as_deref()));
        let format = summarize_media_format(&rep.media);
        let is_mono = variants.contains(&"Mono")
            || edition_label.to_ascii_lowercase().contains("mono");
        let is_alt_mix = variants.contains(&"Alt Mix");
        let is_vinyl = format
            .as_deref()
            .is_some_and(|f| f.to_ascii_lowercase().contains("vinyl"));
        let is_sacd = format.as_deref().is_some_and(|f| {
            let l = f.to_ascii_lowercase();
            l.contains("sacd") || l.contains("dualdisc")
        });
        let is_alternate = !has_default && (is_mono || is_alt_mix || is_vinyl || is_sacd || title_mismatched);

        out.push(ReleaseEditionOption {
            id: rep.id.clone(),
            title: rep.title.clone(),
            edition_label,
            year,
            track_count: total_tracks,
            disc_count,
            format,
            is_default: has_default,
            is_alternate,
            source_kind: Some("musicbrainz".to_string()),
            url: Some(format!("canonical:{}", rep.id)),
        });
    }

    // Single-disc CD releases are hidden when another visible non-alternate edition exists
    // with the same track count (keep 2xCD+ or unique-track-count CDs visible).
    let visible_track_counts: std::collections::HashSet<usize> = out
        .iter()
        .filter(|e| {
            if e.is_alternate {
                return false;
            }
            let is_cd = e.format.as_deref().is_some_and(|f| {
                let l = f.to_ascii_lowercase();
                l.contains("cd") || l.contains("sacd") || l.contains("dualdisc")
            });
            e.is_default || !is_cd || e.disc_count >= 2
        })
        .map(|e| e.track_count)
        .collect();

    for e in &mut out {
        if e.is_default || e.is_alternate {
            continue;
        }
        let is_cd = e.format.as_deref().is_some_and(|f| {
            let l = f.to_ascii_lowercase();
            l.contains("cd") || l.contains("sacd") || l.contains("dualdisc")
        });
        if is_cd && e.disc_count < 2 && visible_track_counts.contains(&e.track_count) {
            e.is_alternate = true;
        }
    }

    // Never leave the default list completely empty when every release is vinyl/mono/retitled.
    if out.iter().all(|e| e.is_alternate) {
        for e in &mut out {
            e.is_alternate = false;
        }
    }

    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.is_alternate.cmp(&b.is_alternate))
            .then_with(|| a.track_count.cmp(&b.track_count))
            .then_with(|| a.year.unwrap_or(9999).cmp(&b.year.unwrap_or(9999)))
            .then_with(|| a.edition_label.cmp(&b.edition_label))
    });
    out
}

/// Picks a MusicBrainz release-group matching `(artist, album_title)` when upgrading a platform album.
pub fn pick_matching_release_group(
    artist: &str,
    album_title: &str,
    groups: &[MbReleaseGroupSummary],
) -> Option<String> {
    let target_artist = crate::search::ranking::normalize(artist);
    let target_title = strip_trailing_bracketed_edition_noise(album_title);
    let raw_target_title = crate::search::ranking::normalize(album_title);
    if target_artist.is_empty() || raw_target_title.is_empty() {
        return None;
    }

    let mut best: Option<(i32, u32, &MbReleaseGroupSummary)> = None;
    for rg in groups {
        if is_bootleg_only_group(&rg.releases) {
            continue;
        }
        let (rg_artist_str, _) = format_artist_credit(&rg.artist_credit);
        let rg_artist = crate::search::ranking::normalize(&rg_artist_str);
        if rg_artist.is_empty() || rg_artist != target_artist {
            continue;
        }

        let rg_raw_title = crate::search::ranking::normalize(&rg.title);
        let rg_core_title = strip_trailing_bracketed_edition_noise(&rg.title);
        let title_rank = if rg_raw_title == raw_target_title {
            30
        } else if !target_title.is_empty() && rg_core_title == target_title {
            20
        } else {
            continue;
        };

        let type_bonus = match rg.primary_type.as_deref() {
            Some(p) if p.eq_ignore_ascii_case("Album") && rg.secondary_types.is_empty() => 15,
            Some(p) if p.eq_ignore_ascii_case("Album") => 8,
            Some(p) if p.eq_ignore_ascii_case("EP") => 5,
            _ => 0,
        };
        let total_score = title_rank + type_bonus;
        let editions = rg.count.unwrap_or(0);
        if best.map_or(true, |(bs, be, _)| (total_score, editions) > (bs, be)) {
            best = Some((total_score, editions, rg));
        }
    }

    best.map(|(_, _, rg)| rg.id.clone())
}

/// Seeds MusicBrainz canonical release tracks with initial platform candidates parsed from `raw`.
pub fn seed_candidates_from_preview(
    release: &mut CanonicalRelease,
    raw: &crate::download::album_preview::AlbumPreview,
) {
    use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
    use crate::types::Platform;

    let platform_tracks: Vec<crate::download::TrackDownload> = raw
        .tracks
        .iter()
        .filter(|t| t.available && !t.url.is_empty())
        .map(|t| crate::download::TrackDownload {
            url: t.url.clone(),
            title: t.title.clone(),
            artists: t.artists.clone(),
            album_artist: raw.artist.clone(),
            album: raw.title.clone(),
            track_number: t.track_number,
            disc_number: t.disc_number,
            duration_ms: t.duration_ms,
            ..crate::download::TrackDownload::default()
        })
        .collect();
    if platform_tracks.is_empty() {
        return;
    }

    for disc in &mut release.discs {
        for track in &mut disc.tracks {
            if let Some(pt) = crate::resolver::matcher::find_matching_track_for_disc(
                &track.title,
                track.duration_ms,
                track.track_number,
                Some(disc.disc_number),
                &platform_tracks,
            ) {
                let cand_dur = pt.duration_ms.unwrap_or(0);
                let eval = crate::resolver::matcher::evaluate_track_match(
                    &track.title,
                    track.duration_ms,
                    &pt.title,
                    cand_dur,
                );
                let (plat_str, tier, codec, bitrate, sample_rate, display_label) =
                    match raw.platform {
                        Platform::Bandcamp if raw.is_free_download => (
                            "bandcamp",
                            AudioQualityTier::Lossless,
                            "FLAC",
                            None,
                            Some(44100),
                            "Bandcamp · FLAC (Lossless)",
                        ),
                        Platform::Bandcamp => (
                            "bandcamp",
                            AudioQualityTier::StandardLossy,
                            "MP3",
                            Some(128),
                            Some(44100),
                            "Bandcamp (MP3 128k)",
                        ),
                        Platform::Soundcloud => (
                            "soundcloud",
                            AudioQualityTier::StandardLossy,
                            "MP3",
                            Some(128),
                            Some(44100),
                            "SoundCloud (MP3 128k)",
                        ),
                        Platform::Youtube | Platform::Local => (
                            "youtube",
                            AudioQualityTier::HighLossy,
                            "Opus",
                            Some(160),
                            Some(48000),
                            "YouTube Music (Opus 160k)",
                        ),
                    };
                let matched_artist = pt
                    .artists
                    .first()
                    .filter(|s| !s.trim().is_empty())
                    .cloned()
                    .or_else(|| (!pt.album_artist.trim().is_empty()).then(|| pt.album_artist.clone()));
                track.candidates.push(AudioCandidate {
                    source: AudioSource::HttpStream {
                        platform: plat_str.to_string(),
                        url: pt.url.clone(),
                        format_id: None,
                        direct_stream_url: None,
                        expires_at: None,
                    },
                    quality_tier: tier,
                    codec: codec.to_string(),
                    bitrate_kbps: bitrate,
                    sample_rate_hz: sample_rate,
                    duration_ms: cand_dur,
                    match_confidence: eval.confidence,
                    is_strict_match: eval.is_strict_match,
                    display_label: display_label.to_string(),
                    matched_title: Some(pt.title.clone()),
                    matched_artist,
                });
                crate::resolver::arbiter::rank_candidates(&mut track.candidates);
                if track.duration_ms == 0 && cand_dur > 0 {
                    track.duration_ms = cand_dur;
                }
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// MusicBrainz JSON DTOs
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MbReleaseGroupSearchResponse {
    #[serde(default, rename = "release-groups")]
    pub release_groups: Vec<MbReleaseGroupSummary>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MbReleaseStubMedium {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default, rename = "track-count")]
    pub track_count: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MbReleaseStub {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub disambiguation: Option<String>,
    #[serde(default)]
    pub media: Vec<MbReleaseStubMedium>,
}

#[derive(Debug, Deserialize)]
pub struct MbReleaseListResponse {
    #[serde(default)]
    pub releases: Vec<MbReleaseStub>,
}

#[derive(Debug, Deserialize)]
pub struct MbReleaseGroupRef {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct MbReleaseGroupDetails {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub releases: Vec<MbReleaseStub>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MbReleaseGroupSummary {
    pub id: String,
    pub title: String,
    #[serde(default, rename = "first-release-date")]
    pub first_release_date: Option<String>,
    #[serde(default, rename = "primary-type")]
    pub primary_type: Option<String>,
    #[serde(default, rename = "secondary-types")]
    pub secondary_types: Vec<String>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub releases: Vec<MbReleaseStub>,
    #[serde(default)]
    pub score: Option<u32>,
}

impl MbReleaseGroupSummary {
    pub fn into_canonical_summary(self) -> CanonicalReleaseSummary {
        let (artist, _) = format_artist_credit(&self.artist_credit);
        let year = self
            .first_release_date
            .as_deref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<u32>().ok());
        let cover_url = format!(
            "https://coverartarchive.org/release-group/{}/front-250",
            self.id
        );
        CanonicalReleaseSummary {
            id: format!("rg:{}", self.id),
            mbid: Some(self.id),
            title: self.title,
            artist,
            year,
            track_count: 0,
            cover_url: Some(cover_url),
            metadata_source: "musicbrainz".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MbArtistCredit {
    pub name: String,
    pub joinphrase: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MbReleaseDetails {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default, rename = "release-group")]
    pub release_group: Option<MbReleaseGroupRef>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub media: Vec<MbMedium>,
}

#[derive(Debug, Deserialize)]
pub struct MbMedium {
    pub position: u32,
    pub title: Option<String>,
    #[serde(default)]
    pub tracks: Vec<MbTrack>,
}

#[derive(Debug, Deserialize)]
pub struct MbTrack {
    pub position: u32,
    pub title: String,
    #[serde(default)]
    pub length: Option<u64>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub recording: Option<MbRecording>,
}

#[derive(Debug, Deserialize)]
pub struct MbRecording {
    pub id: String,
    pub title: Option<String>,
    #[serde(default)]
    pub length: Option<u64>,
}

// -------------------------------------------------------------------------------------------------
// Parsing & Mapping Logic
// -------------------------------------------------------------------------------------------------

pub fn format_artist_credit(credits: &[MbArtistCredit]) -> (String, Vec<String>) {
    if credits.is_empty() {
        return ("Unknown Artist".to_string(), vec!["Unknown Artist".to_string()]);
    }

    let mut full = String::new();
    let mut names = Vec::new();

    for credit in credits {
        full.push_str(&credit.name);
        if let Some(ref join) = credit.joinphrase {
            full.push_str(join);
        }
        names.push(credit.name.clone());
    }

    (full, names)
}

pub fn parse_mb_release_details(raw: MbReleaseDetails, cover_url: Option<String>) -> CanonicalRelease {
    let (album_artist, _) = format_artist_credit(&raw.artist_credit);
    let is_va = album_artist.eq_ignore_ascii_case("Various Artists");

    let release_year = raw
        .date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|y| y.parse::<u32>().ok());

    let mut discs = Vec::new();
    let mut total_tracks = 0;

    for medium in raw.media {
        let mut tracks = Vec::new();

        for track in medium.tracks {
            let (_track_artist_full, track_artists) = format_artist_credit(&track.artist_credit);
            let artists = if track_artists.is_empty() || (track_artists.len() == 1 && track_artists[0] == "Unknown Artist") {
                vec![album_artist.clone()]
            } else {
                track_artists
            };

            let duration_ms = track
                .length
                .or_else(|| track.recording.as_ref().and_then(|r| r.length))
                .unwrap_or(0);

            tracks.push(CanonicalTrack {
                track_number: track.position,
                disc_number: medium.position,
                title: track.title,
                artists,
                duration_ms,
                isrc: None,
                candidates: Vec::new(),
                selected_candidate_idx: 0,
            });
            total_tracks += 1;
        }

        discs.push(CanonicalDisc {
            disc_number: medium.position,
            title: medium.title.filter(|t| !t.trim().is_empty()),
            tracks,
        });
    }

    let total_discs = discs.len();
    let release_group_id = raw.release_group.map(|g| g.id);

    CanonicalRelease {
        id: raw.id.clone(),
        mbid: Some(raw.id),
        release_group_id,
        title: raw.title,
        album_artist,
        is_various_artists: is_va,
        release_year,
        total_tracks,
        total_discs,
        discs,
        cover_url,
        metadata_source: "musicbrainz".to_string(),
    }
}

fn sanitize_lucene_phrase(s: &str) -> String {
    s.chars()
        .map(|c| {
            if matches!(
                c,
                '+' | '-' | '&' | '|' | '!' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '"' | '~' | '*' | '?' | ':' | '\\' | '/'
            ) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Converts a raw user search query into a MusicBrainz Lucene query:
/// - strips intent words ("album", "albums", "ep", "lp", "discography", "songs", "tracks")
///   unless doing so empties the query
/// - strips Lucene special operators so special punctuation never causes HTTP 400
/// - leaves terms unquoted so `<artist> <album>` matches across artist credit and release title
/// - expands colloquial aliases ("white album" by Beatles -> includes "the beatles")
pub fn build_mb_lucene_query(query: &str) -> String {
    let cleaned_chars: String = query
        .chars()
        .map(|c| {
            if matches!(
                c,
                '+' | '-' | '&' | '|' | '!' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '"' | '~' | '*' | '?' | ':' | '\\' | '/'
            ) {
                ' '
            } else {
                c
            }
        })
        .collect();

    let raw_words: Vec<&str> = cleaned_chars.split_whitespace().collect();
    if raw_words.is_empty() {
        return String::new();
    }

    let stripped: Vec<&str> = raw_words
        .iter()
        .copied()
        .filter(|w| {
            !matches!(
                w.to_ascii_lowercase().as_str(),
                "album" | "albums" | "ep" | "lp" | "discography" | "song" | "songs" | "track" | "tracks" | "full"
            )
        })
        .collect();

    let terms = if stripped.is_empty() {
        raw_words
    } else {
        stripped
    };

    // Expand "the beatle" singular stem or "white album" colloquial alias
    let lower_joined = terms.join(" ").to_ascii_lowercase();
    if (lower_joined.contains("beatle") || lower_joined.contains("beatles"))
        && query.to_ascii_lowercase().contains("white")
    {
        return r#"(artist:"The Beatles" AND (release:"The Beatles" OR comment:"white album"))"#.to_string();
    }

    terms
        .into_iter()
        .map(|w| if w.eq_ignore_ascii_case("beatle") { "beatles" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mb_lucene_query_strips_intent_and_escapes_operators() {
        assert_eq!(build_mb_lucene_query("death grips no love deep web"), "death grips no love deep web");
        assert_eq!(build_mb_lucene_query("the doors albums"), "the doors");
        assert_eq!(build_mb_lucene_query("A! (B): [C]?"), "A B C");
        assert!(build_mb_lucene_query("the beatle white album").contains("The Beatles"));
    }

    #[test]
    fn parses_single_artist_multi_disc_release() {
        let json = r#"{
            "id": "mbid-1234",
            "title": "Selected Ambient Works Volume II",
            "date": "1994-03-07",
            "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }],
            "media": [
                {
                    "position": 1,
                    "title": "Disc 1",
                    "tracks": [
                        { "position": 1, "title": "Cliffs", "length": 444000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] },
                        { "position": 2, "title": "Radiator", "length": 394000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] }
                    ]
                },
                {
                    "position": 2,
                    "title": "Disc 2",
                    "tracks": [
                        { "position": 1, "title": "Blue Calx", "length": 440000, "artist-credit": [{ "name": "Aphex Twin", "joinphrase": null }] }
                    ]
                }
            ]
        }"#;

        let details: MbReleaseDetails = serde_json::from_str(json).unwrap();
        let release = parse_mb_release_details(details, Some("https://cover.test/front.jpg".into()));

        assert_eq!(release.id, "mbid-1234");
        assert_eq!(release.mbid.as_deref(), Some("mbid-1234"));
        assert_eq!(release.title, "Selected Ambient Works Volume II");
        assert_eq!(release.album_artist, "Aphex Twin");
        assert!(!release.is_various_artists);
        assert_eq!(release.release_year, Some(1994));
        assert_eq!(release.total_discs, 2);
        assert_eq!(release.total_tracks, 3);
        assert_eq!(release.discs[0].tracks.len(), 2);
        assert_eq!(release.discs[1].tracks.len(), 1);
        assert_eq!(release.discs[0].tracks[0].title, "Cliffs");
        assert_eq!(release.discs[0].tracks[0].duration_ms, 444000);
        assert_eq!(release.cover_url.as_deref(), Some("https://cover.test/front.jpg"));
    }

    #[test]
    fn parses_various_artists_compilation() {
        let json = r#"{
            "id": "mbid-va-8mile",
            "title": "8 Mile: Music from and Inspired by the Motion Picture",
            "date": "2002-10-29",
            "artist-credit": [{ "name": "Various Artists", "joinphrase": null }],
            "media": [
                {
                    "position": 1,
                    "title": null,
                    "tracks": [
                        { "position": 1, "title": "Lose Yourself", "length": 326000, "artist-credit": [{ "name": "Eminem", "joinphrase": null }] },
                        { "position": 2, "title": "Love Me", "length": 270000, "artist-credit": [{ "name": "Obie Trice", "joinphrase": ", " }, { "name": "50 Cent", "joinphrase": " & " }, { "name": "Eminem", "joinphrase": null }] }
                    ]
                }
            ]
        }"#;

        let details: MbReleaseDetails = serde_json::from_str(json).unwrap();
        let release = parse_mb_release_details(details, None);

        assert!(release.is_various_artists);
        assert_eq!(release.album_artist, "Various Artists");
        assert_eq!(release.discs[0].tracks[0].artists, vec!["Eminem"]);
        assert_eq!(release.discs[0].tracks[1].artists, vec!["Obie Trice", "50 Cent", "Eminem"]);
    }

    #[test]
    fn release_group_parsing_and_preferred_release_selection() {
        let json = r#"{
            "release-groups": [
                {
                    "id": "deee6f92-2662-3095-8aea-6892b10b8265",
                    "score": 100,
                    "count": 35,
                    "title": "Morrison Hotel",
                    "first-release-date": "1970-02-20",
                    "primary-type": "Album",
                    "secondary-types": [],
                    "artist-credit": [{ "name": "The Doors", "joinphrase": null }],
                    "releases": [
                        { "id": "bootleg-1", "title": "Morrison Hotel", "status": "Bootleg" },
                        { "id": "sessions-1", "title": "Morrison Hotel Sessions", "status": "Official" },
                        { "id": "official-clean", "title": "Morrison Hotel", "status": "Official" }
                    ]
                },
                {
                    "id": "bootleg-group",
                    "score": 74,
                    "count": 1,
                    "title": "Morrison",
                    "first-release-date": "1992-06-02",
                    "primary-type": "Album",
                    "secondary-types": ["Compilation"],
                    "artist-credit": [{ "name": "The Doors", "joinphrase": null }],
                    "releases": [
                        { "id": "b-rel", "title": "Morrison", "status": "Bootleg" }
                    ]
                }
            ]
        }"#;

        let parsed: MbReleaseGroupSearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.release_groups.len(), 2);

        let mh = parsed.release_groups[0].clone();
        assert_eq!(
            pick_preferred_release_id(&mh.title, &mh.releases).as_deref(),
            Some("official-clean")
        );
        assert!(!is_bootleg_only_group(&mh.releases));

        let summary = mh.into_canonical_summary();
        assert_eq!(summary.id, "rg:deee6f92-2662-3095-8aea-6892b10b8265");
        assert_eq!(summary.year, Some(1970));
        assert_eq!(
            summary.cover_url.as_deref(),
            Some("https://coverartarchive.org/release-group/deee6f92-2662-3095-8aea-6892b10b8265/front-250")
        );

        let boot = &parsed.release_groups[1];
        assert!(is_bootleg_only_group(&boot.releases));
    }

    #[test]
    fn pick_preferred_release_id_prefers_earliest_dated_official_edition() {
        let releases = vec![
            MbReleaseStub {
                id: "reissue-2026".to_string(),
                title: "Drukqs".to_string(),
                status: Some("Official".to_string()),
                date: Some("2026-11-13".to_string()),
                ..Default::default()
            },
            MbReleaseStub {
                id: "undated-official".to_string(),
                title: "Drukqs".to_string(),
                status: Some("Official".to_string()),
                date: None,
                ..Default::default()
            },
            MbReleaseStub {
                id: "original-2001".to_string(),
                title: "Drukqs".to_string(),
                status: Some("Official".to_string()),
                date: Some("2001-10-22".to_string()),
                ..Default::default()
            },
        ];
        assert_eq!(
            pick_preferred_release_id("Drukqs", &releases).as_deref(),
            Some("original-2001")
        );
    }

    fn stub(
        id: &str,
        title: &str,
        disambig: Option<&str>,
        status: Option<&str>,
        date: Option<&str>,
        country: Option<&str>,
        media: Vec<(&str, usize)>,
    ) -> MbReleaseStub {
        MbReleaseStub {
            id: id.to_string(),
            title: title.to_string(),
            status: status.map(str::to_string),
            date: date.map(str::to_string),
            country: country.map(str::to_string),
            disambiguation: disambig.map(str::to_string),
            media: media
                .into_iter()
                .map(|(fmt, count)| MbReleaseStubMedium {
                    format: Some(fmt.to_string()),
                    track_count: count,
                })
                .collect(),
        }
    }

    #[test]
    fn pick_preferred_release_prefers_cd_over_digital_and_4x_vinyl_on_same_date() {
        let releases = vec![
            stub("vinyl-4", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("GB"), vec![("12\" Vinyl", 6), ("12\" Vinyl", 5), ("12\" Vinyl", 12), ("12\" Vinyl", 7)]),
            stub("digital-1", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("XW"), vec![("Digital Media", 30)]),
            stub("cd-2", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("GB"), vec![("CD", 15), ("CD", 15)]),
        ];
        assert_eq!(pick_preferred_release_id("Drukqs", &releases).as_deref(), Some("cd-2"));
    }

    #[test]
    fn group_releases_collapses_drukqs_20_pressings_into_6_distinct_versions() {
        let drukqs = vec![
            stub("0c13023d", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("XW"), vec![("Digital Media", 30)]),
            stub("23496fd1", "Drukqs", None, Some("Official"), Some("2001"), Some("AU"), vec![("CD", 15), ("CD", 15)]),
            stub("3fd9698f", "Drukqs", Some("expanded edition"), Some("Official"), Some("2026-11-13"), Some("CA"), vec![("Digital Media", 35)]),
            stub("4d35da90", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("GB"), vec![("12\" Vinyl", 6), ("12\" Vinyl", 5), ("12\" Vinyl", 12), ("12\" Vinyl", 7)]),
            stub("50a4e953", "Drukqs", None, Some("Official"), Some("2017-07-20"), Some("XW"), vec![("Digital Media", 31)]),
            stub("5fc41f19", "Drukqs", Some("35 track reissue"), Some("Official"), Some("2026-10-30"), Some("XW"), vec![("Digital Media", 35)]),
            stub("60fa0e80", "Drukqs", Some("Bleep download"), Some("Official"), None, Some("XW"), vec![("Digital Media", 32)]),
            stub("62729b63", "Drukqs", Some("25th anniversary reissue"), Some("Official"), Some("2026-11-13"), None, vec![("CD", 15), ("CD", 15)]),
            stub("88224d97", "Drukqs", None, Some("Promotion"), Some("2001"), Some("JP"), vec![("CD", 15), ("CD", 15)]),
            stub("98693ac9", "Drukqs", Some("Limited Edition, Numbered, RTI Pressing"), Some("Official"), Some("2002-09-09"), Some("GB"), vec![("12\" Vinyl", 30)]),
            stub("9dd65019", "Drukqs", None, Some("Official"), Some("2018-12-09"), Some("XW"), vec![("Digital Media", 34)]),
            stub("a3a96dde", "Drukqs", None, Some("Official"), Some("2001-10-23"), Some("US"), vec![("CD", 15), ("CD", 15)]),
            stub("aab378a0", "Drukqs", None, None, Some("2001"), Some("RU"), vec![("CD", 15), ("CD", 15)]),
            stub("cf08cadb", "Drukqs", None, Some("Official"), Some("2014"), Some("XE"), vec![("CD", 15), ("CD", 15)]),
            stub("dbf15355", "Drukqs", None, Some("Official"), Some("2017-07-28"), Some("XE"), vec![("CD", 15), ("CD", 15)]),
            stub("e46b74be", "Drukqs", Some("25th anniversary reissue"), Some("Official"), Some("2026-11-13"), None, vec![("Cassette", 15), ("Cassette", 15)]),
            stub("e527b82a", "Drukqs", None, Some("Official"), Some("2018-12-03"), Some("XW"), vec![("Digital Media", 34)]),
            stub("efc8db20", "Drukqs", Some("25th anniversary reissue"), Some("Official"), Some("2026-11-13"), None, vec![("12\" Vinyl", 30)]),
            stub("f1e1e043", "Drukqs", None, Some("Official"), Some("2018-12-04"), Some("XW"), vec![("Digital Media", 33)]),
            stub("facbe59c", "Drukqs", None, Some("Official"), Some("2001-10-22"), Some("GB"), vec![("CD", 15), ("CD", 15)]),
        ];

        let grouped = group_releases_for_picker("Drukqs", &drukqs);
        let counts: Vec<usize> = grouped.iter().map(|g| g.track_count).collect();
        assert_eq!(counts, vec![30, 31, 32, 33, 34, 35]);

        assert!(grouped[0].is_default);
        assert_eq!(grouped[0].id, "facbe59c");
        assert_eq!(grouped[0].disc_count, 2);
        assert_eq!(grouped[0].year, Some(2001));
        assert_eq!(grouped[0].edition_label, "Original edition");

        assert_eq!(grouped[2].id, "60fa0e80");
        assert_eq!(grouped[2].edition_label, "Bleep download");

        assert_eq!(grouped[5].edition_label, "Expanded edition");
    }

    #[test]
    fn group_releases_keeps_content_variants_separate_at_identical_track_counts() {
        let releases = vec![
            stub("explicit-us", "The Money Store", Some("explicit"), Some("Official"), Some("2012-04-21"), Some("US"), vec![("Digital Media", 13)]),
            stub("explicit-uk", "The Money Store", None, Some("Official"), Some("2012-04-24"), Some("GB"), vec![("CD", 13)]),
            stub("explicit-vinyl", "The Money Store", Some("180g gatefold reissue"), Some("Official"), Some("2022"), Some("US"), vec![("12\" Vinyl", 13)]),
            stub("clean-us", "The Money Store", Some("clean"), Some("Official"), Some("2012-04-24"), Some("US"), vec![("Digital Media", 13)]),
            stub("inst-digital", "The Money Store (Instrumentals)", None, Some("Official"), Some("2012"), Some("XW"), vec![("Digital Media", 13)]),
            stub("bootleg-1", "The Money Store", Some("unofficial"), Some("Bootleg"), Some("2015"), Some("DE"), vec![("12\" Vinyl", 14)]),
        ];

        let grouped = group_releases_for_picker("The Money Store", &releases);
        assert_eq!(grouped.len(), 3, "got: {grouped:?}");
        assert!(grouped[0].is_default);
        assert_eq!(grouped[0].id, "explicit-us");
        let labels: Vec<&str> = grouped.iter().map(|g| g.edition_label.as_str()).collect();
        assert!(labels.contains(&"Original edition"));
        assert!(labels.contains(&"Clean"));
        assert!(labels.contains(&"Instrumental"));
        assert!(!grouped.iter().any(|g| g.id == "bootleg-1"));
    }

    #[test]
    fn group_releases_keeps_mono_acoustic_and_title_editions_while_dropping_remaster_clones() {
        let releases = vec![
            stub("orig-stereo", "In Utero", None, Some("Official"), Some("1993-09-21"), Some("US"), vec![("CD", 12)]),
            stub("remaster-2013", "In Utero (20th Anniversary Remaster)", Some("2013 remaster"), Some("Official"), Some("2013"), Some("XW"), vec![("Digital Media", 12)]),
            stub("mix-2013", "In Utero", Some("2013 mix"), Some("Official"), Some("2013"), Some("XW"), vec![("Digital Media", 12)]),
            stub("mono-uk", "In Utero", Some("mono"), Some("Official"), Some("1994"), Some("GB"), vec![("CD", 12)]),
            stub("acoustic", "In Utero (Unplugged)", None, Some("Official"), Some("1995"), Some("US"), vec![("CD", 12)]),
            stub("deluxe", "In Utero (Super Deluxe)", None, Some("Official"), Some("2013"), Some("US"), vec![("CD", 12), ("CD", 18), ("CD", 13)]),
        ];
        let grouped = group_releases_for_picker("In Utero", &releases);
        // Expected: Original (collapses remaster-2013 into orig-stereo!), 2013 mix, Mono, Acoustic (12), Super Deluxe (43)
        assert_eq!(grouped.len(), 5, "got: {grouped:?}");
        assert_eq!(grouped[0].id, "orig-stereo");
        assert!(grouped.iter().any(|g| g.id == "mix-2013"));
        assert!(grouped.iter().any(|g| g.id == "mono-uk" && g.edition_label == "Mono"));
        assert!(grouped.iter().any(|g| g.id == "acoustic" && g.edition_label == "Acoustic"));
        assert!(grouped.iter().any(|g| g.id == "deluxe" && g.track_count == 43));
    }

    #[test]
    fn group_releases_falls_back_to_bootlegs_when_no_official_exists() {
        let releases = vec![stub("boot-1", "Rare Tape", None, Some("Bootleg"), Some("1999"), Some("XW"), vec![("CD", 8)])];
        let grouped = group_releases_for_picker("Rare Tape", &releases);
        assert_eq!(grouped.len(), 1);
        assert!(grouped[0].is_default);
        assert_eq!(grouped[0].id, "boot-1");
    }

    #[test]
    fn pick_matching_release_group_finds_studio_album_and_rejects_mismatches() {
        let groups = vec![
            MbReleaseGroupSummary {
                id: "rg-single".into(),
                title: "The Money Store".into(),
                first_release_date: Some("2012".into()),
                primary_type: Some("Single".into()),
                secondary_types: vec![],
                artist_credit: vec![MbArtistCredit { name: "Death Grips".into(), joinphrase: None }],
                count: Some(2),
                releases: vec![],
                score: Some(100),
            },
            MbReleaseGroupSummary {
                id: "rg-album".into(),
                title: "The Money Store".into(),
                first_release_date: Some("2012-04-21".into()),
                primary_type: Some("Album".into()),
                secondary_types: vec![],
                artist_credit: vec![MbArtistCredit { name: "Death Grips".into(), joinphrase: None }],
                count: Some(24),
                releases: vec![],
                score: Some(100),
            },
            MbReleaseGroupSummary {
                id: "rg-wrong-artist".into(),
                title: "The Money Store".into(),
                first_release_date: Some("2020".into()),
                primary_type: Some("Album".into()),
                secondary_types: vec![],
                artist_credit: vec![MbArtistCredit { name: "Other Band".into(), joinphrase: None }],
                count: Some(1),
                releases: vec![],
                score: Some(100),
            },
        ];

        assert_eq!(
            pick_matching_release_group("Death Grips", "The Money Store (Explicit)", &groups).as_deref(),
            Some("rg-album")
        );
        assert_eq!(
            pick_matching_release_group("Unknown Indie", "The Money Store", &groups),
            None
        );
    }

    #[test]
    fn seed_candidates_from_preview_attaches_youtube_tracks_and_duration() {
        use crate::download::album_preview::{AlbumPreview, AlbumTrackInfo};
        use crate::types::Platform;

        let mut release = CanonicalRelease {
            id: "rg:tms".into(),
            mbid: Some("rel-tms".into()),
            release_group_id: Some("tms".into()),
            title: "The Money Store".into(),
            album_artist: "Death Grips".into(),
            is_various_artists: false,
            release_year: Some(2012),
            total_tracks: 1,
            total_discs: 1,
            discs: vec![CanonicalDisc {
                disc_number: 1,
                title: None,
                tracks: vec![CanonicalTrack {
                    track_number: 1,
                    disc_number: 1,
                    title: "Get Got".into(),
                    artists: vec!["Death Grips".into()],
                    duration_ms: 0,
                    isrc: None,
                    candidates: vec![],
                    selected_candidate_idx: 0,
                }],
            }],
            cover_url: None,
            metadata_source: "musicbrainz".into(),
        };

        let raw = AlbumPreview {
            platform: Platform::Youtube,
            url: "https://music.youtube.com/browse/MPREb_tms".into(),
            title: "The Money Store".into(),
            artist: "Death Grips".into(),
            cover_url: None,
            year: Some(2012),
            genres: vec![],
            tracks: vec![AlbumTrackInfo {
                index: 0,
                title: "Get Got".into(),
                artists: vec!["Death Grips".into()],
                track_number: Some(1),
                disc_number: Some(1),
                url: "https://music.youtube.com/watch?v=getgot".into(),
                available: true,
                note: None,
                duration_ms: Some(172_000),
                candidates: vec![],
                selected_candidate_idx: 0,
            }],
            total_duration_ms: Some(172_000),
            metadata_source: "youtube".into(),
            is_free_download: false,
            release_group_id: None,
            selected_release_id: None,
            candidates_resolved: false,
        };

        seed_candidates_from_preview(&mut release, &raw);
        assert_eq!(release.discs[0].tracks[0].candidates.len(), 1);
        assert_eq!(
            release.discs[0].tracks[0].candidates[0].source.primary_url(),
            "https://music.youtube.com/watch?v=getgot"
        );
        assert_eq!(release.discs[0].tracks[0].duration_ms, 172_000);
    }

    #[test]
    fn release_lookup_url_uses_plural_release_groups_inc_param() {
        let url = release_lookup_url("0c13023d-0ccf-4376-8634-4c86504eefad");
        assert!(url.contains("inc=recordings+artist-credits+media+release-groups&fmt=json"));
    }

    #[test]
    fn group_releases_marks_vinyl_mono_sacd_alt_mix_and_alt_titles_as_alternate() {
        let releases = vec![
            stub("doors-vinyl", "The Doors", None, Some("Official"), Some("1967-01-04"), Some("US"), vec![("12\" Vinyl", 11)]),
            stub("doors-mono", "The Doors", Some("mono"), Some("Official"), Some("1967"), Some("US"), vec![("12\" Vinyl", 11)]),
            stub("doors-hdcd", "The Doors", Some("HDCD encoded"), Some("Official"), Some("1999"), Some("US"), vec![("CD", 11)]),
            stub("doors-cd", "The Doors", None, Some("Official"), None, Some("US"), vec![("CD", 11)]),
            stub("doors-lmf", "Light My Fire", None, Some("Official"), Some("1988"), Some("DE"), vec![("CD", 11)]),
            stub("doors-atmos", "The Doors", Some("Dolby Atmos mix"), Some("Official"), Some("2022"), Some("XW"), vec![("Digital Media", 11)]),
            stub("doors-40th", "The Doors", Some("Expanded 40th anniversary mixes"), Some("Official"), Some("2007"), Some("US"), vec![("CD", 14)]),
            stub("doors-sacd", "The Doors", Some("AP remaster"), Some("Official"), Some("2013"), Some("US"), vec![("Hybrid SACD (CD layer)", 11), ("Hybrid SACD (CD layer)", 11), ("Hybrid SACD (CD layer)", 11)]),
            stub("doors-4cd", "The Doors", Some("50th anniversary deluxe edition"), Some("Official"), Some("2017"), Some("US"), vec![("CD", 11), ("CD", 11), ("CD", 11), ("CD", 8)]),
        ];

        let grouped = group_releases_for_picker("The Doors", &releases);
        let def = grouped.iter().find(|g| g.is_default).unwrap();
        // Standard 1-disc 11-track CD beats 3xHybrid SACD remaster (33 tracks) and 1967 Vinyl
        assert_eq!(def.id, "doors-cd");
        assert_eq!(def.track_count, 11);
        assert_eq!(def.disc_count, 1);
        assert_eq!(def.edition_label, "Original edition");
        assert!(!def.is_alternate);

        // Mono, Dolby Atmos mix, Hybrid SACD layer clone, and alt-titled "Light My Fire" go under "Show all releases"
        assert!(grouped.iter().find(|g| g.id == "doors-mono").unwrap().is_alternate);
        assert!(grouped.iter().find(|g| g.id == "doors-atmos").unwrap().is_alternate);
        assert!(grouped.iter().find(|g| g.id == "doors-sacd").unwrap().is_alternate);
        let lmf = grouped.iter().find(|g| g.id == "doors-lmf").unwrap();
        assert!(lmf.is_alternate);
        assert_eq!(lmf.edition_label, "Light My Fire");

        // 14-track unique CD and 4xCD deluxe stay visible
        assert!(!grouped.iter().find(|g| g.id == "doors-40th").unwrap().is_alternate);
        assert!(!grouped.iter().find(|g| g.id == "doors-4cd").unwrap().is_alternate);
    }

    #[test]
    fn group_releases_keeps_vinyl_or_mono_visible_when_no_other_releases_exist() {
        let vinyl_only = vec![
            stub("lp-only", "Wax Only", Some("mono"), Some("Official"), Some("1968"), Some("US"), vec![("12\" Vinyl", 10)]),
        ];
        let grouped = group_releases_for_picker("Wax Only", &vinyl_only);
        assert_eq!(grouped.len(), 1);
        assert!(!grouped[0].is_alternate);
    }

    #[test]
    fn pick_matching_release_group_rejects_partial_artist_matches() {
        let groups = vec![
            MbReleaseGroupSummary {
                id: "rg-tribute".into(),
                title: "The Soft Parade".into(),
                first_release_date: Some("2010".into()),
                primary_type: Some("Album".into()),
                secondary_types: vec![],
                artist_credit: vec![MbArtistCredit {
                    name: "The Doors Tribute Band".into(),
                    joinphrase: None,
                }],
                count: Some(5),
                releases: vec![],
                score: Some(100),
            },
        ];

        assert_eq!(
            pick_matching_release_group("The Doors", "The Soft Parade", &groups),
            None
        );

        let with_exact = [
            groups,
            vec![MbReleaseGroupSummary {
                id: "rg-soft-parade".into(),
                title: "The Soft Parade".into(),
                first_release_date: Some("1969-07-18".into()),
                primary_type: Some("Album".into()),
                secondary_types: vec![],
                artist_credit: vec![MbArtistCredit {
                    name: "The Doors".into(),
                    joinphrase: None,
                }],
                count: Some(42),
                releases: vec![],
                score: Some(100),
            }],
        ]
        .concat();

        assert_eq!(
            pick_matching_release_group("The Doors", "The Soft Parade", &with_exact).as_deref(),
            Some("rg-soft-parade")
        );
    }

    #[test]
    fn try_cached_canonical_for_album_reads_disk_cache_without_network() {
        let temp_dir = std::env::temp_dir().join(format!("bt_mb_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let client = MusicBrainzClient::new(Some(temp_dir.clone()));

        let cached_release = CanonicalRelease {
            id: "rel-soft-parade".into(),
            mbid: Some("rel-soft-parade".into()),
            release_group_id: Some("rg-soft-parade".into()),
            title: "The Soft Parade".into(),
            album_artist: "The Doors".into(),
            is_various_artists: false,
            release_year: Some(1969),
            total_tracks: 9,
            total_discs: 1,
            discs: vec![CanonicalDisc {
                disc_number: 1,
                title: None,
                tracks: vec![CanonicalTrack {
                    track_number: 1,
                    disc_number: 1,
                    title: "Tell All the People".into(),
                    artists: vec!["The Doors".into()],
                    duration_ms: 201_000,
                    isrc: None,
                    candidates: vec![],
                    selected_candidate_idx: 0,
                }],
            }],
            cover_url: None,
            metadata_source: "musicbrainz".to_string(),
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let found = rt.block_on(async {
            client
                .write_cache("rg_match_v1:the doors::the soft parade", "rg-soft-parade")
                .await;
            client
                .write_cache("rg_release_v7:rg-soft-parade", "rel-soft-parade")
                .await;
            client
                .write_cache(
                    "release_v7:rel-soft-parade",
                    &serde_json::to_string(&cached_release).unwrap(),
                )
                .await;
            client
                .try_cached_canonical_for_album("The Doors", "The Soft Parade")
                .await
        });

        let _ = std::fs::remove_dir_all(&temp_dir);
        let rel = found.expect("expected cached canonical release");
        assert_eq!(rel.id, "rg:rg-soft-parade");
        assert_eq!(rel.mbid.as_deref(), Some("rel-soft-parade"));
        assert_eq!(rel.title, "The Soft Parade");
    }

    #[tokio::test]
    async fn superseded_request_fails_before_connecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = MusicBrainzClient::new(None).only_while(|| false);

        let err = client
            .get_json_with_retry(&format!("http://127.0.0.1:{port}/ws/2/release-group/?fmt=json"), "test")
            .await
            .expect_err("a superseded request must not succeed");

        assert!(err.contains("replaced by a newer one"), "unexpected error: {err}");
        match listener.accept() {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "no connection expected"),
            Ok(_) => panic!("superseded request connected to the server"),
        }
    }

    fn saved_release_groups_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt_mb_saved_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const SAVED_RELEASE_GROUPS_JSON: &str =
        r#"{"release-groups":[{"id":"rg-saved","title":"Saved Album","primary-type":"Album"}]}"#;

    #[tokio::test]
    async fn saved_release_groups_is_none_without_saved_answer() {
        let dir = saved_release_groups_test_dir("missing");
        let client = MusicBrainzClient::new(Some(dir.clone()));

        assert!(client.saved_release_groups("death grips").await.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn saved_release_groups_reads_answer_saved_under_search_key() {
        let dir = saved_release_groups_test_dir("fresh");
        let client = MusicBrainzClient::new(Some(dir.clone()));
        let key = MusicBrainzClient::release_group_cache_key(&build_mb_lucene_query("Death Grips"));
        assert_eq!(key, "rg_search_v2:death grips");
        client.write_cache(&key, SAVED_RELEASE_GROUPS_JSON).await;

        let groups = client
            .saved_release_groups("Death Grips")
            .await
            .expect("saved answer should be found");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "rg-saved");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn saved_release_groups_still_returns_answer_older_than_cache_ttl() {
        let dir = saved_release_groups_test_dir("expired");
        let client = MusicBrainzClient::new(Some(dir.clone()));
        let key = MusicBrainzClient::release_group_cache_key(&build_mb_lucene_query("Death Grips"));
        client.write_cache(&key, SAVED_RELEASE_GROUPS_JSON).await;
        let path = client.cache_path(&key).expect("cache path");
        let thirty_days_ago = std::time::SystemTime::now() - Duration::from_secs(30 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(thirty_days_ago)
            .unwrap();

        assert!(client.read_cache(&key).await.is_none(), "read_cache must expire old entries");
        let groups = client
            .saved_release_groups("Death Grips")
            .await
            .expect("saved answer should be used even when old");
        assert_eq!(groups[0].id, "rg-saved");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
