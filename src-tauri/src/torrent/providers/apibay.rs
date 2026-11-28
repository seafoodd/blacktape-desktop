use super::*;

pub(super) fn request(client: &Client, query: &Query, title_only: bool) -> RequestBuilder {
    let text = query.text(title_only);
    client
        .get("https://apibay.org/q.php")
        .query(&[("q", text.as_str()), ("cat", "100")])
}

pub(in crate::torrent::search) fn parse(
    root: &Value,
    query: &Query,
) -> Result<Vec<SearchHit>, ProviderError> {
    let rows = root.as_array().ok_or(ProviderError::Malformed)?;
    let candidates = rows
        .iter()
        .take(MAX_ROWS)
        .filter(|row| count(row.get("id")) != Some(0));
    parse_rows(candidates, |row| {
        count(row.get("id"))?;
        let title = title(row, "name")?;
        let rank = query.identity(&title, &[]);
        let hash = normalize_info_hash(row.get("info_hash")?.as_str()?).ok()?;
        let hit = magnet_hit(
            Provider::ApiBay,
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
