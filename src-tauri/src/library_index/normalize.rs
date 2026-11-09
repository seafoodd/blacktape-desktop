//! Url normalization used to compare remote urls with `songs.source_url` and `album_sources.album_key`.
//! Owner: part A. Pure and heavily tested (see PLAN.md for the exact rules).

use crate::types::Platform;
use url::{Host, Url};

/// Host prefixes that never identify a different page (mobile / www / youtube music).
const IGNORED_HOST_PREFIXES: [&str; 3] = ["www.", "m.", "music."];

/// Comparable key of a page url, or None when `url` is empty/unparsable. Rules (D3):
/// trim; drop scheme, `www.`/`m.`/`music.` host prefixes, query and fragment, trailing slashes; lowercase the
/// rest. YouTube (youtube.com, music.youtube.com, youtu.be): key `yt:<video id>` for watch/shorts/youtu.be urls
/// and `ytlist:<list id>` for playlist/album urls.
///
/// Deviations that are deliberate: YouTube ids keep their case (they are case-sensitive, lowercasing would make
/// different videos collide), and a prefix is not stripped when it would turn an artist subdomain into the bare
/// `bandcamp.com` (`music.bandcamp.com` is an artist called "music").
pub fn normalize_url_key(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        format!("https://{trimmed}")
    };
    let parsed = Url::parse(&with_scheme).ok()?;
    // Page urls carry no credentials; this also rejects "mailto:a@b.com", which parses as userinfo.
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }

    let host = match parsed.host()? {
        Host::Domain(domain) => {
            let domain = strip_host_prefixes(&domain.to_lowercase()).to_string();
            // A bare word ("garbage") is not a page url.
            if !domain.contains('.') {
                return None;
            }
            domain
        }
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{ip}]"),
    };

    if let Some(key) = youtube_key(&host, &parsed) {
        return Some(key);
    }

    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = parsed.path().trim_end_matches('/').to_lowercase();
    Some(format!("{host}{port}{path}"))
}

/// Fallback key from `songs.source_item_id` (only YouTube ids are comparable): `youtube:<id>`.
pub fn item_id_key(platform: Platform, source_item_id: &str) -> Option<String> {
    let id = source_item_id.trim();
    if id.is_empty() || platform != Platform::Youtube {
        None
    } else {
        Some(format!("youtube:{id}"))
    }
}

fn strip_host_prefixes(host: &str) -> &str {
    let mut current = host;
    loop {
        let mut stripped = false;
        for prefix in IGNORED_HOST_PREFIXES {
            if let Some(rest) = current.strip_prefix(prefix) {
                if rest.contains('.') && rest != "bandcamp.com" {
                    current = rest;
                    stripped = true;
                    break;
                }
            }
        }
        if !stripped {
            return current;
        }
    }
}

fn query_value(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, v)| k == name && !v.trim().is_empty())
        .map(|(_, v)| v.trim().to_string())
}

/// `yt:<id>` / `ytlist:<id>` for the url shapes of YouTube, None for every other url (or a youtube url that
/// is not a video/playlist, e.g. a channel, which then falls back to the generic key).
fn youtube_key(host: &str, url: &Url) -> Option<String> {
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    match host {
        "youtu.be" => segments.first().map(|id| format!("yt:{id}")),
        "youtube.com" => match segments.first().copied() {
            Some("watch") => query_value(url, "v")
                .map(|id| format!("yt:{id}"))
                .or_else(|| query_value(url, "list").map(|list| format!("ytlist:{list}"))),
            Some("shorts" | "embed" | "live" | "v") => {
                segments.get(1).map(|id| format!("yt:{id}"))
            }
            Some("playlist") => query_value(url, "list").map(|list| format!("ytlist:{list}")),
            Some("browse") => segments.get(1).map(|id| format!("ytlist:{id}")),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(url: &str) -> Option<String> {
        normalize_url_key(url)
    }

    #[test]
    fn bandcamp_track_and_album_urls() {
        assert_eq!(
            key("https://artist.bandcamp.com/track/Some-Song").as_deref(),
            Some("artist.bandcamp.com/track/some-song")
        );
        assert_eq!(
            key("https://artist.bandcamp.com/album/the-album").as_deref(),
            Some("artist.bandcamp.com/album/the-album")
        );
    }

    #[test]
    fn bandcamp_trailing_slash_query_and_fragment_are_ignored() {
        let base = key("https://artist.bandcamp.com/track/x");
        assert_eq!(key("https://artist.bandcamp.com/track/x/"), base);
        assert_eq!(key("https://artist.bandcamp.com/track/x?from=embed"), base);
        assert_eq!(key("https://artist.bandcamp.com/track/x/?from=embed#t=3"), base);
        assert_eq!(key("https://artist.bandcamp.com/track/x///"), base);
    }

    #[test]
    fn bandcamp_uppercase_and_scheme_are_ignored() {
        let base = key("https://artist.bandcamp.com/track/x");
        assert_eq!(key("HTTPS://ARTIST.BANDCAMP.COM/Track/X"), base);
        assert_eq!(key("http://artist.bandcamp.com/track/x"), base);
    }

    #[test]
    fn bandcamp_subdomains_are_different_keys() {
        assert_ne!(
            key("https://a.bandcamp.com/track/x"),
            key("https://b.bandcamp.com/track/x")
        );
    }

    #[test]
    fn bandcamp_artist_called_like_a_prefix_is_not_collapsed() {
        assert_eq!(
            key("https://music.bandcamp.com/track/x").as_deref(),
            Some("music.bandcamp.com/track/x")
        );
        assert_eq!(
            key("https://m.bandcamp.com/album/y").as_deref(),
            Some("m.bandcamp.com/album/y")
        );
    }

    #[test]
    fn soundcloud_mobile_host_and_tracking_query() {
        let base = key("https://soundcloud.com/artist/track-name");
        assert_eq!(base.as_deref(), Some("soundcloud.com/artist/track-name"));
        assert_eq!(key("https://m.soundcloud.com/artist/track-name"), base);
        assert_eq!(key("https://www.soundcloud.com/artist/track-name"), base);
        assert_eq!(
            key("https://soundcloud.com/artist/track-name?si=abc123&utm_source=clipboard"),
            base
        );
        assert_eq!(key("https://soundcloud.com/Artist/Track-Name/"), base);
    }

    #[test]
    fn soundcloud_sets_keep_their_path() {
        assert_eq!(
            key("https://soundcloud.com/artist/sets/the-ep").as_deref(),
            Some("soundcloud.com/artist/sets/the-ep")
        );
    }

    #[test]
    fn youtube_watch_urls() {
        assert_eq!(key("https://www.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("yt:dQw4w9WgXcQ"));
        assert_eq!(key("https://youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("yt:dQw4w9WgXcQ"));
        assert_eq!(key("https://m.youtube.com/watch?v=dQw4w9WgXcQ").as_deref(), Some("yt:dQw4w9WgXcQ"));
    }

    #[test]
    fn youtube_watch_with_extra_params() {
        assert_eq!(
            key("https://www.youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42s&list=PLabc").as_deref(),
            Some("yt:dQw4w9WgXcQ"),
            "the video wins over the list"
        );
        assert_eq!(
            key("https://www.youtube.com/watch?v=dQw4w9WgXcQ#comments").as_deref(),
            Some("yt:dQw4w9WgXcQ")
        );
    }

    #[test]
    fn youtube_ids_keep_their_case() {
        assert_ne!(key("https://youtu.be/AbCdEfGhIjK"), key("https://youtu.be/abcdefghijk"));
        assert_eq!(key("https://youtu.be/AbCdEfGhIjK").as_deref(), Some("yt:AbCdEfGhIjK"));
    }

    #[test]
    fn youtube_music_host_gives_the_same_key() {
        assert_eq!(
            key("https://music.youtube.com/watch?v=dQw4w9WgXcQ"),
            key("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
        assert_eq!(
            key("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=xyz").as_deref(),
            Some("yt:dQw4w9WgXcQ")
        );
    }

    #[test]
    fn youtu_be_short_links() {
        assert_eq!(key("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some("yt:dQw4w9WgXcQ"));
        assert_eq!(key("https://youtu.be/dQw4w9WgXcQ?t=10").as_deref(), Some("yt:dQw4w9WgXcQ"));
        assert_eq!(key("youtu.be/dQw4w9WgXcQ/").as_deref(), Some("yt:dQw4w9WgXcQ"));
    }

    #[test]
    fn youtube_shorts_embed_and_live() {
        assert_eq!(key("https://www.youtube.com/shorts/abc123XYZ_-").as_deref(), Some("yt:abc123XYZ_-"));
        assert_eq!(key("https://www.youtube.com/embed/abc123XYZ_-").as_deref(), Some("yt:abc123XYZ_-"));
        assert_eq!(key("https://www.youtube.com/live/abc123XYZ_-?feature=share").as_deref(), Some("yt:abc123XYZ_-"));
    }

    #[test]
    fn youtube_playlist_and_browse_urls() {
        assert_eq!(
            key("https://www.youtube.com/playlist?list=PLabc123").as_deref(),
            Some("ytlist:PLabc123")
        );
        assert_eq!(
            key("https://music.youtube.com/playlist?list=OLAK5uy_abc").as_deref(),
            Some("ytlist:OLAK5uy_abc")
        );
        assert_eq!(
            key("https://music.youtube.com/browse/MPREb_abc123").as_deref(),
            Some("ytlist:MPREb_abc123")
        );
        assert_eq!(
            key("https://www.youtube.com/watch?list=PLabc123").as_deref(),
            Some("ytlist:PLabc123"),
            "a watch url without a video falls back to the list"
        );
    }

    #[test]
    fn youtube_urls_without_a_video_use_the_generic_key() {
        assert_eq!(key("https://www.youtube.com/@Someone/").as_deref(), Some("youtube.com/@someone"));
        assert_eq!(key("https://www.youtube.com/watch").as_deref(), Some("youtube.com/watch"));
        assert_eq!(key("https://www.youtube.com/playlist").as_deref(), Some("youtube.com/playlist"));
    }

    #[test]
    fn schemeless_urls_are_accepted() {
        assert_eq!(
            key("artist.bandcamp.com/track/x").as_deref(),
            Some("artist.bandcamp.com/track/x")
        );
        assert_eq!(key("www.youtube.com/watch?v=abc").as_deref(), Some("yt:abc"));
        assert_eq!(key("//soundcloud.com/a/b").as_deref(), Some("soundcloud.com/a/b"));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            key("  https://a.bandcamp.com/track/x \n").as_deref(),
            Some("a.bandcamp.com/track/x")
        );
    }

    #[test]
    fn empty_and_garbage_are_none() {
        assert_eq!(key(""), None);
        assert_eq!(key("   "), None);
        assert_eq!(key("garbage"), None);
        assert_eq!(key("https://"), None);
        assert_eq!(key("not a url at all"), None);
        assert_eq!(key("mailto:someone@example.com"), None);
        assert_eq!(key("javascript:alert(1)"), None);
        assert_eq!(key("file:///C:/music/a.mp3"), None);
        assert_eq!(key("ftp://example.com/a.mp3"), None);
    }

    #[test]
    fn host_only_urls_give_the_host() {
        assert_eq!(key("https://bandcamp.com/").as_deref(), Some("bandcamp.com"));
        assert_eq!(key("https://www.example.com").as_deref(), Some("example.com"));
    }

    #[test]
    fn non_default_port_is_kept() {
        assert_eq!(key("http://example.com:8080/a/").as_deref(), Some("example.com:8080/a"));
        assert_eq!(key("https://example.com:443/a").as_deref(), Some("example.com/a"));
    }

    #[test]
    fn percent_encoded_and_unicode_paths_compare_equal() {
        assert_eq!(
            key("https://a.bandcamp.com/album/caf\u{e9}"),
            key("https://a.bandcamp.com/album/caf%C3%A9")
        );
    }

    #[test]
    fn item_id_key_is_youtube_only() {
        assert_eq!(item_id_key(Platform::Youtube, "dQw4w9WgXcQ").as_deref(), Some("youtube:dQw4w9WgXcQ"));
        assert_eq!(item_id_key(Platform::Youtube, "  abc  ").as_deref(), Some("youtube:abc"));
        assert_eq!(item_id_key(Platform::Bandcamp, "12345"), None);
        assert_eq!(item_id_key(Platform::Soundcloud, "12345"), None);
        assert_eq!(item_id_key(Platform::Local, "12345"), None);
    }

    #[test]
    fn item_id_key_rejects_empty_ids() {
        assert_eq!(item_id_key(Platform::Youtube, ""), None);
        assert_eq!(item_id_key(Platform::Youtube, "   "), None);
    }
}
