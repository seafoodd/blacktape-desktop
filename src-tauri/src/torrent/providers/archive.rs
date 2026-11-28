use super::*;

const CHECK_LIMIT: usize = 6;

fn escape_phrase(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '"'
                | '+'
                | '-'
                | '!'
                | '('
                | ')'
                | ':'
                | '^'
                | '['
                | ']'
                | '{'
                | '}'
                | '~'
                | '*'
                | '?'
                | '/'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub(in crate::torrent::search) fn search_request(client: &Client, query: &Query, title_only: bool) -> RequestBuilder {
    let artist = escape_phrase(&query.artist);
    let title = escape_phrase(&query.title);
    let expression = if title_only {
        format!(r#"mediatype:audio AND title:"{title}""#)
    } else {
        format!(r#"mediatype:audio AND ((creator:"{artist}" AND title:"{title}") OR (title:"{artist}" AND title:"{title}"))"#)
    };
    let rows = MAX_ROWS.to_string();
    client
        .get("https://archive.org/advancedsearch.php")
        .query(&[
            ("q", expression.as_str()),
            ("fl[]", "identifier"),
            ("fl[]", "title"),
            ("fl[]", "creator"),
            ("fl[]", "item_size"),
            ("fl[]", "downloads"),
            ("sort[]", "downloads desc"),
            ("rows", rows.as_str()),
            ("output", "json"),
        ])
}

fn identifier(value: &str) -> Option<&str> {
    (value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')))
    .then_some(value)
}

fn strings(value: Option<&Value>) -> Vec<&str> {
    match value {
        Some(Value::String(value)) => vec![value],
        Some(Value::Array(values)) => values.iter().take(12).filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn torrent_name(id: &str) -> String {
    format!("{id}_archive.torrent")
}

pub(in crate::torrent::search) fn parse_search(
    root: &Value,
    query: &Query,
) -> Result<Vec<SearchHit>, ProviderError> {
    let rows = root
        .pointer("/response/docs")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Malformed)?;
    parse_rows(rows, |row| {
        let id = identifier(row.get("identifier")?.as_str()?)?;
        let creators = strings(row.get("creator"));
        let titles = strings(row.get("title"))
            .into_iter()
            .filter_map(|title| clean_field(title, MAX_RESULT_TITLE_BYTES));
        let mut best = None;
        for title in titles {
            let rank = query.identity(&title, &creators);
            if best.as_ref().is_none_or(|(_, best_rank)| rank > *best_rank) {
                best = Some((title, rank));
            }
        }
        let (mut title, rank) = best?;
        let normalized_title = crate::search::ranking::normalize(&title);
        let title_words = normalized_title.split_whitespace().collect::<Vec<_>>();
        if query.artist_prefix(&title_words).is_none() {
            if let Some(creator) = creators
                .into_iter()
                .find(|creator| query.matches_creator(creator))
            {
                let creator = clean_field(creator, MAX_RESULT_TITLE_BYTES)?;
                title = format!("{creator} – {title}");
            }
        }
        let provider = Provider::Archive.name().to_string();
        Some((
            SearchHit {
                id: format!("archive:{id}"),
                title,
                input: format!("https://archive.org/download/{id}/{}", torrent_name(id)),
                providers: vec![provider.clone()],
                provider,
                info_hash: None,
                seeders: None,
                size_bytes: count(row.get("item_size")),
                popularity: count(row.get("downloads")),
                rank: 0,
            },
            rank,
        ))
    })
}

pub(in crate::torrent::search) fn has_torrent(
    root: &Value,
    id: &str,
) -> Result<bool, ProviderError> {
    if root.get("error").is_some() {
        return Err(ProviderError::Unavailable);
    }
    if root.as_array().is_some_and(Vec::is_empty) {
        return Ok(false);
    }
    let files = root
        .get("files")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Malformed)?;
    let expected = torrent_name(id);
    Ok(files.iter().any(|file| {
        file.get("name").and_then(Value::as_str) == Some(expected.as_str())
            && !file.get("private").is_some_and(|value| {
                value.as_bool() == Some(true) || matches!(value.as_str(), Some("true" | "1"))
            })
    }))
}

pub(super) async fn check_hits(
    client: &Client,
    gate: &Gate,
    deadline: Instant,
    hits: Vec<SearchHit>,
) -> (Vec<SearchHit>, Option<ProviderError>) {
    let mut available = Vec::new();
    let mut error = None;
    for hit in hits.into_iter().take(CHECK_LIMIT) {
        let Some(id) = hit.id.strip_prefix("archive:") else {
            continue;
        };
        let request = client.get(format!("https://archive.org/metadata/{id}"));
        match gate
            .json(request, deadline)
            .await
            .and_then(|root| has_torrent(&root, id))
        {
            Ok(true) => available.push(hit),
            Ok(false) => {}
            Err(current) => {
                error = Some(current);
                if matches!(current, ProviderError::Timeout | ProviderError::RateLimited) {
                    break;
                }
            }
        }
    }
    (available, error)
}
