use crate::types::Platform;

/// Classifies a page by its HTTP(S) host, never by text in its path or query.
pub fn platform_of_url(raw: &str) -> Option<Platform> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.trim_end_matches('.');
    let on_domain = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    if on_domain("youtube.com") || on_domain("youtu.be") {
        Some(Platform::Youtube)
    } else if on_domain("bandcamp.com") {
        Some(Platform::Bandcamp)
    } else if on_domain("soundcloud.com") || on_domain("snd.sc") {
        Some(Platform::Soundcloud)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
