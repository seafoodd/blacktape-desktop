pub(crate) use super::release::InspectedRelease;
#[cfg(test)]
pub(crate) use super::release::ReleaseLayout;
use super::release::{analyze_release, RawFile};
use librqbit::{dht::Id20, torrent_from_bytes, ByteBuf, FileIteratorName};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{header, redirect::Policy, Client};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{watch, Semaphore};
use tokio::task::AbortHandle;
use url::{Host, Url};

#[path = "inspect_peer.rs"]
mod peer;

const INSPECTION_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const EXACT_SOURCE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_INPUT_LENGTH: usize = 8192;
const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 8192;
const MAX_PATH_COMPONENTS: usize = 16;
const MAX_BENCODE_DEPTH: usize = 24;
const MAX_BENCODE_VALUES: usize = 256_000;
const MAX_PENDING_INSPECTIONS: usize = 32;
const MAX_REDIRECTS: usize = 4;

enum InspectionInput {
    TorrentFile(Url),
    Magnet(MagnetInput),
}

struct MagnetInput {
    info_hash: String,
    exact_sources: Vec<Url>,
    peers: Vec<SocketAddr>,
}

type InspectionResult = Result<Arc<InspectedRelease>, String>;

struct RunningInspection {
    result: watch::Receiver<Option<InspectionResult>>,
    task: AbortHandle,
}

impl Drop for RunningInspection {
    fn drop(&mut self) {
        self.task.abort();
    }
}

static INSPECTIONS: LazyLock<Mutex<HashMap<String, Weak<RunningInspection>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CACHED_TORRENT_BYTES: LazyLock<Mutex<Vec<(String, Vec<u8>)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static INSPECTION_SLOTS: Semaphore = Semaphore::const_new(2);
const MAX_CACHED_TORRENT_BYTES: usize = 24;

pub(super) fn cache_torrent_bytes(key: &str, bytes: &[u8]) {
    let norm = key.trim().to_ascii_lowercase();
    if norm.is_empty() || bytes.is_empty() || bytes.len() > MAX_METADATA_BYTES {
        return;
    }
    if let Ok(mut guard) = CACHED_TORRENT_BYTES.lock() {
        guard.retain(|(k, _)| k != &norm);
        if guard.len() >= MAX_CACHED_TORRENT_BYTES {
            guard.remove(0);
        }
        guard.push((norm, bytes.to_vec()));
    }
}

pub(super) fn cached_torrent_bytes(locator: &str) -> Option<Vec<u8>> {
    let key = if let Ok(hash) = magnet_info_hash(locator) {
        hash.to_ascii_lowercase()
    } else {
        locator.trim().to_ascii_lowercase()
    };
    let guard = CACHED_TORRENT_BYTES.lock().ok()?;
    guard
        .iter()
        .rev()
        .find(|(k, _)| k == &key)
        .map(|(_, bytes)| bytes.clone())
}

pub(super) fn normalize_locator_for_download(input: &str) -> Result<(), String> {
    let input = input.trim();
    if input.len() > MAX_INPUT_LENGTH {
        return Err("Torrent locator is too long.".into());
    }
    match Url::parse(input).as_ref().map(Url::scheme) {
        Ok("magnet") => {
            parse_magnet(input)?;
            Ok(())
        }
        _ => {
            parse_http_url(input)?;
            Ok(())
        }
    }
}

pub(super) async fn inspect_with_app(app: Option<&tauri::AppHandle>, input: &str) -> Result<InspectedRelease, String> {
    if let Some(topic) = super::rutracker::topic(input) {
        let app = app.ok_or("RuTracker browser session is unavailable.")?;
        return super::rutracker::torrent(app, topic, None).await.map(|(release, _)| release);
    }
    inspect(input).await
}

pub(crate) async fn inspect(input: &str) -> Result<InspectedRelease, String> {
    let input = input.trim();
    if input.len() > MAX_INPUT_LENGTH {
        return Err("Torrent locator is too long.".into());
    }
    let input = match Url::parse(input).as_ref().map(Url::scheme) {
        Ok("magnet") => InspectionInput::Magnet(parse_magnet(input)?),
        _ => InspectionInput::TorrentFile(parse_http_url(input)?),
    };

    let key = match &input {
        InspectionInput::TorrentFile(url) => url.as_str(),
        InspectionInput::Magnet(magnet) => magnet.info_hash.as_str(),
    };
    let inspection = {
        let mut inspections = INSPECTIONS
            .lock()
            .map_err(|_| "Torrent inspection is unavailable.")?;
        inspections.retain(|_, inspection| inspection.strong_count() != 0);
        if let Some(existing) = inspections.get(key).and_then(Weak::upgrade) {
            existing
        } else {
            if inspections.len() >= MAX_PENDING_INSPECTIONS {
                return Err("Too many torrent inspections are pending.".into());
            }
            let key = key.to_owned();
            let (sender, result) = watch::channel(None);
            let task = tokio::spawn(async move {
                let outcome = tokio::time::timeout(INSPECTION_TIMEOUT, inspect_from_source(input))
                    .await
                    .unwrap_or_else(|_| Err("Torrent metadata inspection timed out.".into()));
                sender.send_replace(Some(outcome.map(Arc::new)));
            });
            let added = Arc::new(RunningInspection {
                result,
                task: task.abort_handle(),
            });
            inspections.insert(key, Arc::downgrade(&added));
            added
        }
    };

    let mut result = inspection.result.clone();
    loop {
        if let Some(outcome) = result.borrow_and_update().as_ref() {
            return outcome.clone().map(|release| (*release).clone());
        }
        result
            .changed()
            .await
            .map_err(|_| "Torrent metadata inspection stopped.")?;
    }
}

async fn inspect_from_source(input: InspectionInput) -> Result<InspectedRelease, String> {
    let _permit = INSPECTION_SLOTS
        .acquire()
        .await
        .map_err(|_| "Torrent inspection is unavailable.")?;
    match input {
        InspectionInput::TorrentFile(url) => {
            let url_key = url.as_str().to_owned();
            let bytes = fetch_metadata(url, false).await?;
            let release = parse_metainfo(&bytes, None)?;
            cache_torrent_bytes(&release.info_hash, &bytes);
            cache_torrent_bytes(&url_key, &bytes);
            Ok(release)
        }
        InspectionInput::Magnet(magnet) => {
            for source in magnet.exact_sources {
                if let Ok(Ok(bytes)) =
                    tokio::time::timeout(EXACT_SOURCE_TIMEOUT, fetch_metadata(source, false)).await
                {
                    if let Ok(release) = parse_metainfo(&bytes, Some(&magnet.info_hash)) {
                        cache_torrent_bytes(&release.info_hash, &bytes);
                        return Ok(release);
                    }
                }
            }
            if let Some(release) = peer::try_peers(magnet.peers, &magnet.info_hash).await {
                return Ok(release);
            }
            // Only send the normalized hash to this public metadata cache.
            let cache_url = Url::parse(&format!(
                "https://itorrents.net/torrent/{}.torrent",
                magnet.info_hash.to_ascii_uppercase(),
            ))
            .map_err(|_| "Could not locate the torrent metadata cache.")?;
            let bytes = fetch_metadata(cache_url, true).await?;
            let release = parse_metainfo(&bytes, Some(&magnet.info_hash))?;
            cache_torrent_bytes(&release.info_hash, &bytes);
            Ok(release)
        }
    }
}

pub(super) fn normalize_info_hash(raw: &str) -> Result<String, String> {
    if !matches!(raw.len(), 32 | 40) || !raw.is_ascii() {
        return Err("Invalid BitTorrent v1 info hash.".into());
    }
    let id: Id20 = raw
        .to_ascii_uppercase()
        .parse()
        .map_err(|_| "Invalid BitTorrent v1 info hash.")?;
    if id == Id20::default() {
        return Err("Invalid BitTorrent v1 info hash.".into());
    }
    Ok(id.as_string())
}

pub(super) fn magnet_info_hash(magnet: &str) -> Result<String, String> {
    parse_magnet(magnet).map(|magnet| magnet.info_hash)
}

fn parse_magnet(input: &str) -> Result<MagnetInput, String> {
    if input.len() > MAX_INPUT_LENGTH {
        return Err("Magnet link is too long.".into());
    }
    let url = Url::parse(input).map_err(|_| "Invalid magnet link.")?;
    if url.scheme() != "magnet"
        || url.has_host()
        || !url.path().is_empty()
        || url.fragment().is_some()
    {
        return Err("Invalid magnet link.".into());
    }
    let mut info_hash = None;
    let mut exact_sources = Vec::new();
    let mut peers = Vec::new();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "xt" => {
                if value
                    .get(..9)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("urn:btih:"))
                {
                    let hash = normalize_info_hash(&value[9..])?;
                    if info_hash.as_ref().is_some_and(|previous| *previous != hash) {
                        return Err(
                            "Magnet link contains conflicting BitTorrent v1 info hashes.".into(),
                        );
                    }
                    info_hash = Some(hash);
                }
            }
            "xs" if exact_sources.len() < 2 => {
                if let Ok(source) = parse_http_url(&value) {
                    if !exact_sources.contains(&source) {
                        exact_sources.push(source);
                    }
                }
            }
            "x.pe" if peers.len() < peer::MAX_EXPLICIT_PEERS => {
                if let Ok(address) = value.parse::<SocketAddr>() {
                    if address.port() != 0
                        && is_public_address(address.ip())
                        && !peers.contains(&address)
                    {
                        peers.push(address);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(MagnetInput {
        info_hash: info_hash.ok_or("Magnet link must contain a BitTorrent v1 info hash.")?,
        exact_sources,
        peers,
    })
}

fn parse_http_url(input: &str) -> Result<Url, String> {
    let url =
        Url::parse(input).map_err(|_| "Enter a magnet link or public HTTP(S) torrent URL.")?;
    validate_http_url(&url)?;
    Ok(url)
}

fn validate_http_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str().len() > MAX_INPUT_LENGTH
    {
        return Err("Enter a public HTTP(S) torrent URL without credentials.".into());
    }
    let valid_host = match url.host() {
        Some(Host::Ipv4(ip)) => is_public_address(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_public_address(IpAddr::V6(ip)),
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.');
            domain.contains('.')
                && !matches!(domain, "localhost" | "local" | "internal")
                && !domain.ends_with(".localhost")
                && !domain.ends_with(".local")
                && !domain.ends_with(".internal")
                && !domain.ends_with(".home.arpa")
        }
        None => false,
    };
    if !valid_host {
        return Err("Torrent URLs must use public network addresses.".into());
    }
    Ok(())
}

fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !matches!(
            ip.octets(),
            [0 | 10 | 127 | 240..=255, ..]
                | [100, 64..=127, ..]
                | [169, 254, ..]
                | [172, 16..=31, ..]
                | [192, 0, 0 | 2, _]
                | [192, 88, 99, _]
                | [192, 168, ..]
                | [198, 18 | 19, ..]
                | [198, 51, 100, _]
                | [203, 0, 113, _]
                | [224..=239, ..]
        ),
        // Only ordinary global unicast IPv6, excluding special-purpose, documentation and tunnel ranges.
        IpAddr::V6(ip) => {
            matches!(ip.segments()[0], 0x2000..=0x3fff)
                && !matches!(
                    ip.segments(),
                    [0x2001, 0x0000..=0x01ff | 0x0db8, ..]
                        | [0x2002, ..]
                        | [0x3fff, 0x0000..=0x0fff, ..]
                )
        }
    }
}

fn validate_resolved(addresses: &[SocketAddr]) -> Result<(), String> {
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !is_public_address(address.ip()))
    {
        return Err("Torrent host did not resolve exclusively to public network addresses.".into());
    }
    Ok(())
}

struct PinnedResolver {
    host: String,
    addresses: Vec<SocketAddr>,
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let result: Result<Addrs, _> = if name.as_str().eq_ignore_ascii_case(&self.host) {
            Ok(Box::new(self.addresses.clone().into_iter()))
        } else {
            Err(std::io::Error::other("Torrent host has no validated addresses.").into())
        };
        Box::pin(std::future::ready(result))
    }
}

pub(super) async fn public_client(url: &Url) -> Result<Client, String> {
    let mut builder = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(5))
        .timeout(REQUEST_TIMEOUT);
    if let Some(Host::Domain(domain)) = url.host() {
        let port = url
            .port_or_known_default()
            .ok_or("Torrent URL does not specify a supported port.")?;
        let mut addresses: Vec<_> = tokio::net::lookup_host((domain, port))
            .await
            .map_err(|_| "Could not resolve the torrent host.")?
            .collect();
        validate_resolved(&addresses)?;
        addresses.truncate(16);
        builder = builder.dns_resolver(Arc::new(PinnedResolver {
            host: domain.to_owned(),
            addresses,
        }));
    }
    builder
        .build()
        .map_err(|_| "Could not start a torrent metadata request.".into())
}

async fn fetch_metadata(mut url: Url, is_cache: bool) -> Result<Vec<u8>, String> {
    for redirects in 0..=MAX_REDIRECTS {
        validate_http_url(&url)?;
        if is_cache
            && (url.scheme() != "https"
                || !matches!(
                    url.host_str(),
                    Some(
                        "itorrents.net"
                            | "www.itorrents.net"
                            | "itorrents.org"
                            | "www.itorrents.org"
                    )
                ))
        {
            return Err("Torrent metadata cache redirected outside its supported hosts.".into());
        }
        let mut response = public_client(&url)
            .await?
            .get(url.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "Torrent metadata request timed out."
                } else {
                    "Could not connect to the torrent metadata host."
                }
            })?;
        if response.status().is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err("Torrent URL redirected too many times.".into());
            }
            let target = response
                .headers()
                .get(header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .and_then(|location| url.join(location).ok())
                .ok_or("Torrent metadata host returned an invalid redirect.")?;
            if url.scheme() == "https" && target.scheme() != "https" {
                return Err("Torrent metadata host redirected to an insecure URL.".into());
            }
            url = target;
            continue;
        }
        if !response.status().is_success() {
            return Err(if is_cache {
                "The public metadata cache has no usable torrent for this magnet. Try its torrent-file URL."
            } else {
                "Torrent metadata host did not return a torrent file."
            }.into());
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
        {
            return Err("Torrent metadata exceeds the size limit.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Could not read the torrent metadata.")?
        {
            if bytes.len() + chunk.len() > MAX_METADATA_BYTES {
                return Err("Torrent metadata exceeds the size limit.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(bytes);
    }
    Err("Torrent URL redirected too many times.".into())
}

pub(super) fn parse_metainfo(bytes: &[u8], expected_hash: Option<&str>) -> Result<InspectedRelease, String> {
    if bytes.len() > MAX_METADATA_BYTES {
        return Err("Torrent metadata exceeds the size limit.".into());
    }
    // librqbit 8.1.1 does not bound nesting or string length arithmetic in its bencode decoder.
    if bencode_prefix(bytes).is_none_or(|remaining| !remaining.is_empty()) {
        return Err("Torrent file contains invalid or unsupported metadata.".into());
    }
    let meta = torrent_from_bytes::<ByteBuf>(bytes)
        .map_err(|_| "Torrent file contains invalid or unsupported BitTorrent v1 metadata.")?;
    let info_hash = meta.info_hash.as_string();
    if expected_hash.is_some_and(|expected| expected != info_hash) {
        return Err("Torrent metadata does not match the magnet's info hash.".into());
    }
    let info = meta.info;
    let name = info
        .name
        .as_ref()
        .map(|name| decode_component(name.as_ref()))
        .transpose()?
        .unwrap_or_else(|| "Unknown release".into());
    let mut files = Vec::new();
    let mut paths = HashSet::new();
    let mut raw_paths = HashSet::new();
    let mut total_size = 0u64;
    let details = info
        .iter_file_details()
        .map_err(|_| "Torrent file has an invalid file list.")?;
    for (index, file) in details.enumerate() {
        if index >= MAX_FILE_COUNT {
            return Err("Torrent metadata contains too many files.".into());
        }
        let attributes = file.attrs();
        if attributes.symlink || file.symlink_path.is_some() {
            return Err("Torrent file contains an unsupported symbolic link.".into());
        }
        let parts = match file.filename {
            FileIteratorName::Single(Some(ref name)) => std::slice::from_ref(*name),
            FileIteratorName::Single(None) => return Err("Torrent file has no filename.".into()),
            FileIteratorName::Tree(parts) => parts,
        };
        if parts.is_empty() || parts.len() > MAX_PATH_COMPONENTS {
            return Err("Torrent file has an invalid path depth.".into());
        }
        if !raw_paths.insert(parts.iter().map(|part| part.as_ref()).collect::<Vec<_>>()) {
            return Err("Torrent file contains duplicate paths.".into());
        }
        let components = parts
            .iter()
            .map(|part| decode_component(part.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut path = components.join("/");
        if parts
            .iter()
            .any(|part| std::str::from_utf8(part.as_ref()).is_err())
        {
            path.push_str(&format!(" [file {index}]"));
        }
        if !paths.insert(path.to_lowercase()) {
            return Err("Torrent file contains duplicate paths.".into());
        }
        total_size = total_size
            .checked_add(file.len)
            .ok_or("Torrent file sizes overflow.")?;
        files.push(RawFile {
            index,
            components,
            path,
            length: file.len,
            padding: attributes.padding,
        });
    }
    if info.piece_length == 0
        || total_size
            .div_ceil(u64::from(info.piece_length))
            .checked_mul(20)
            != Some(info.pieces.as_ref().len() as u64)
    {
        return Err("Torrent file has invalid piece metadata.".into());
    }
    Ok(analyze_release(info_hash, name, total_size, files))
}

fn decode_component(bytes: &[u8]) -> Result<String, String> {
    let part = String::from_utf8_lossy(bytes);
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        });
    if bytes.is_empty()
        || bytes.len() > 255
        || reserved
        || part.ends_with(['.', ' '])
        || part.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err("Torrent file contains an unsafe filename or folder.".into());
    }
    Ok(part.into_owned())
}

fn bencode_prefix(mut bytes: &[u8]) -> Option<&[u8]> {
    fn byte_string(bytes: &mut &[u8]) -> Option<()> {
        let colon = bytes.iter().take(9).position(|byte| *byte == b':')?;
        let length_bytes = bytes.get(..colon)?;
        if length_bytes.is_empty() || !length_bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let length: usize = std::str::from_utf8(length_bytes).ok()?.parse().ok()?;
        *bytes = bytes.get((colon + 1).checked_add(length)?..)?;
        Some(())
    }
    fn value(bytes: &mut &[u8], depth: usize, remaining_values: &mut usize) -> Option<()> {
        if depth > MAX_BENCODE_DEPTH {
            return None;
        }
        *remaining_values = remaining_values.checked_sub(1)?;
        match bytes.first()? {
            b'd' | b'l' => {
                let dict = bytes.first() == Some(&b'd');
                *bytes = bytes.get(1..)?;
                while bytes.first() != Some(&b'e') {
                    if dict {
                        byte_string(bytes)?;
                    }
                    value(bytes, depth + 1, remaining_values)?;
                }
                *bytes = bytes.get(1..)?;
            }
            b'i' => {
                let end = bytes.iter().take(22).position(|byte| *byte == b'e')?;
                let digits = bytes.get(1..end)?;
                let digits = digits.strip_prefix(b"-").unwrap_or(digits);
                if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
                    return None;
                }
                *bytes = bytes.get(end + 1..)?;
            }
            b'0'..=b'9' => byte_string(bytes)?,
            _ => return None,
        }
        Some(())
    }
    if bytes.first() != Some(&b'd') {
        return None;
    }
    let mut remaining_values = MAX_BENCODE_VALUES;
    value(&mut bytes, 0, &mut remaining_values)?;
    Some(bytes)
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
