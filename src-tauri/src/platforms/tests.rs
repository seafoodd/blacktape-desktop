use crate::{platforms::platform_of_url, types::Platform};

#[test]
fn recognizes_supported_platform_hosts() {
    let cases = [
        ("https://youtube.com/watch?v=1", Some(Platform::Youtube)),
        (" https://WWW.YouTube.COM./watch?v=1 ", Some(Platform::Youtube)),
        ("https://youtu.be/abc", Some(Platform::Youtube)),
        ("https://music.youtube.com/watch?v=1", Some(Platform::Youtube)),
        ("https://bandcamp.com/track/x", Some(Platform::Bandcamp)),
        ("https://artist.bandcamp.com/track/x", Some(Platform::Bandcamp)),
        ("https://soundcloud.com/artist/track", Some(Platform::Soundcloud)),
        ("https://api.soundcloud.com/tracks/1", Some(Platform::Soundcloud)),
        ("https://snd.sc/abc", Some(Platform::Soundcloud)),
    ];

    for (url, expected) in cases {
        assert_eq!(platform_of_url(url), expected, "URL: {url}");
    }
}

#[test]
fn rejects_untrusted_host_lookalikes_and_non_http_urls() {
    let cases = [
        "https://evil.com/?next=bandcamp.com",
        "https://notbandcamp.com/track/x",
        "https://bandcamp.com.evil.com/track/x",
        "https://youtube.com.evil.com/watch?v=1",
        "https://soundcloud.com.evil.com/track",
        "https://evil.com/youtube.com/bandcamp.com/soundcloud.com#youtu.be",
        "https://youtube.com@evil.com/watch?v=1",
        "not a url",
        "https://",
        "ftp://youtube.com/file",
        "magnet:?xt=urn:btih:abc&dn=bandcamp.com",
    ];

    for url in cases {
        assert_eq!(platform_of_url(url), None, "URL: {url}");
    }
}
