use super::*;

pub(in crate::torrent::search) fn request(
    client: &Client,
    query: &Query,
    title_only: bool,
) -> RequestBuilder {
    client
        .get("https://solidtorrents.eu/api/v1/search")
        .query(&[("q", query.text(title_only))])
        .query(&[("limit", MAX_ROWS)])
}

pub(in crate::torrent::search) fn parse(
    root: &Value,
    query: &Query,
) -> Result<Vec<SearchHit>, ProviderError> {
    match root.get("success").and_then(Value::as_bool) {
        Some(true) => {}
        Some(false) => return Err(ProviderError::Unavailable),
        None => return Err(ProviderError::Malformed),
    }
    let rows = root
        .get("results")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Malformed)?;
    parse_rows(rows, |row| {
        let title = title(row, "title")?;
        let hash = normalize_info_hash(row.get("infohash")?.as_str()?).ok()?;
        // Complete albums also appear under Other, including the Image subcategory.
        // Keep those candidates and let Blacktape's torrent inspection verify the files.
        let rank = match (count(row.get("category"))?, count(row.get("subCategory"))) {
            (1, Some(2)) | (7, Some(4)) => None,
            (1 | 7, _) => query.identity(&title, &[]),
            _ => None,
        };
        let hit = magnet_hit(
            Provider::SolidTorrents,
            title,
            hash,
            None,
            None,
            count(row.get("seeders")),
            count(row.get("size")),
        );
        Some((hit, rank))
    })
}
