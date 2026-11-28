use super::*;
use regex::Regex;
use std::sync::LazyLock;

static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<tr\b[^>]*>(.*?)</tr>").unwrap());
static MAGNET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bhref\s*=\s*"magnet:\?xt=urn:btih:([a-f0-9]{40})(?:&[^"]*)?""#).unwrap()
});
static RELEASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a\b[^>]*\bhref\s*=\s*"/torrent/[0-9]+/[^"]*"[^>]*>(.*?)</a>"#).unwrap()
});
static DOWNLOAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bhref\s*=\s*"(?:https:)?//d\.rutor\.info/download/([0-9]+)""#).unwrap()
});
static SEEDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<span\b[^>]*class="green"[^>]*>\s*(?:<img\b[^>]*>\s*)?(?:&nbsp;|\s)*([0-9]+)"#).unwrap()
});
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());

pub(super) fn request(client: &Client, query: &Query, title_only: bool) -> RequestBuilder {
    let mut url = Url::parse("https://rutor.info/search/0/0/100/0").unwrap();
    url.path_segments_mut().unwrap().push(&query.text(title_only)).push("");
    client.get(url)
}

pub(super) fn parse(html: &str, query: &Query) -> Result<Vec<SearchHit>, ProviderError> {
    if !html.contains("<title>rutor.info :: Поиск</title>") {
        return Err(ProviderError::Malformed);
    }
    let mut hits = Vec::new();
    for row in ROW.captures_iter(html).filter_map(|row| row.get(1)).map(|row| row.as_str())
        .filter(|row| row.contains("magnet:?xt=")).take(MAX_ROWS)
    {
        let (Some(hash), Some(title)) = (MAGNET.captures(row), RELEASE.captures(row)) else {
            continue;
        };
        let title = TAG.replace_all(&title[1], "");
        let title = html_escape::decode_html_entities(&title);
        let Some(title) = clean_field(&title, MAX_RESULT_TITLE_BYTES) else { continue };
        let Some(rank) = query.identity(&title, &[]) else { continue };
        let exact_source = DOWNLOAD.captures(row)
            .and_then(|parts| Url::parse(&format!("https://d.rutor.info/download/{}", &parts[1])).ok());
        let seeders = SEEDS.captures(row).and_then(|parts| parts[1].parse().ok());
        let mut hit = magnet_hit(
            Provider::Rutor, title, hash[1].to_ascii_lowercase(), None, exact_source, seeders, None,
        );
        hit.rank = rank;
        hits.push(hit);
    }
    Ok(merge_hits(hits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_single_encoded_query_segment() {
        let query = Query::new("A+B", "One / Two").unwrap();
        let request = request(&Client::new(), &query, false).build().unwrap();
        let url = request.url().as_str();
        assert!(url.starts_with("https://rutor.info/search/0/0/100/0/"));
        assert_eq!(request.url().path_segments().unwrap().count(), 7);
    }

    #[test]
    fn parses_music_rows_and_only_keeps_the_known_metadata_host() {
        let page = r#"<title>rutor.info :: Поиск</title>
        <tr class="tum"><td><a class="downgif" href="//d.rutor.info/download/378998">D</a>
        <a href="magnet:?xt=urn:btih:846d70d446a55f72d4a79ad9055ab67d5be40f29&tr=http://127.0.0.1/announce">M</a>
        <a href="/torrent/378998/aphex-twin-syro"><b>Aphex Twin</b> - Syro (2014) FLAC </a>
        </td><td><span class="green"><img alt="S" />&nbsp;3</span></td></tr>
        <tr><td><a href="//d.rutor.info.evil.test/download/123">D</a>
        <a href="magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">M</a>
        <a href="/torrent/123/alternate">Aphex Twin - Syro FLAC</a></td></tr>
        <tr><td><a href="magnet:?xt=urn:btih:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb">M</a>
        <a href="/torrent/55/unrelated">Different Artist - Syro FLAC</a></td></tr>"#;
        let hits = parse(page, &Query::new("Aphex Twin", "Syro").unwrap()).unwrap();
        assert_eq!(hits.len(), 2);
        let hit = hits.iter().find(|hit| hit.seeders == Some(3)).unwrap();
        assert_eq!(hit.provider, "RuTor");
        assert_eq!(hit.title, "Aphex Twin - Syro (2014) FLAC");
        assert!(hit.input.contains("xs=https%3A%2F%2Fd.rutor.info%2Fdownload%2F378998"));
        assert!(hits.iter().all(|hit| !hit.input.contains("evil.test") && !hit.input.contains("127.0.0.1")));
    }

    #[test]
    fn distinguishes_empty_search_from_an_unexpected_page() {
        let query = Query::new("Aphex Twin", "Syro").unwrap();
        assert!(parse("<title>rutor.info :: Поиск</title>", &query).unwrap().is_empty());
        assert!(matches!(parse("<title>Just a moment...</title>", &query), Err(ProviderError::Malformed)));
    }
}
