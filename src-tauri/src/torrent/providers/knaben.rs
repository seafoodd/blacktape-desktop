use super::*;
use sha2::{Digest, Sha256};

pub(super) fn request(client: &Client, query: &Query, title_only: bool) -> RequestBuilder {
    client
        .post("https://api.knaben.org/v1")
        .json(&serde_json::json!({
            "query": query.text(title_only),
        "search_field": "title",
        "categories": [1_000_000], // Audio and its subcategories.
            "order_by": "seeders",
            "size": MAX_ROWS,
        "hide_xxx": true,
        }))
}

fn download_link(row: &Value) -> Option<Url> {
    let raw = row.get("link")?.as_str()?;
    if raw.len() > 2048 {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("knaben.eu" | "knaben.org" | "knaben.xyz")
        )
        && url.port().is_none()
        && url.path().starts_with("/live/dl/")
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

fn optional_string<'a>(row: &'a Value, key: &str) -> Option<Option<&'a str>> {
    match row.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(value) => Some(Some(value.as_str()?).filter(|value| !value.trim().is_empty())),
    }
}

pub(in crate::torrent::search) fn parse(
    root: &Value,
    query: &Query,
) -> Result<Vec<SearchHit>, ProviderError> {
    let rows = root
        .get("hits")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Malformed)?;
    parse_rows(rows, |row| {
        let title = title(row, "title")?;
        let rank = query.identity(&title, &[]);
        let magnet = optional_string(row, "magnetUrl")?;
        let mut hashes = Vec::new();
        if let Some(magnet) = magnet {
            hashes.push(magnet_info_hash(magnet).ok()?);
        }
        for key in ["hash", "infoHash"] {
            if let Some(hash) = optional_string(row, key)? {
                hashes.push(normalize_info_hash(hash).ok()?);
            }
        }
        let seeders = count(row.get("seeders"));
        let size_bytes = count(row.get("bytes"));
        let link = download_link(row);
        let hit = if let Some(hash) = hashes.pop() {
            if hashes.iter().any(|candidate| candidate != &hash) {
                return None;
            }
            magnet_hit(
                Provider::Knaben,
                title,
                hash,
                magnet,
                link,
                seeders,
                size_bytes,
            )
        } else {
            let input = link?.to_string();
            let provider = Provider::Knaben.name().to_string();
            SearchHit {
                id: format!("knaben:{:x}", Sha256::digest(input.as_bytes())),
                title,
                input,
                providers: vec![provider.clone()],
                provider,
                info_hash: None,
                seeders,
                size_bytes,
                popularity: None,
                rank: 0,
            }
        };
        Some((hit, rank))
    })
}
