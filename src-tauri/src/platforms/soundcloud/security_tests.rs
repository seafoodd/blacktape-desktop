use super::build_api_url;

#[test]
fn api_url_accepts_only_the_exact_soundcloud_api_origin() {
    let valid = build_api_url("https://api-v2.soundcloud.com/search/tracks", &[], "client")
        .expect("official API URL should be accepted");
    assert_eq!(valid.host_str(), Some("api-v2.soundcloud.com"));

    for raw in [
        "http://api-v2.soundcloud.com/search",
        "https://soundcloud.com/api",
        "https://evil.com/?next=https://api-v2.soundcloud.com",
        "https://api-v2.soundcloud.com.evil.com/search",
        "https://user@api-v2.soundcloud.com/search",
        "https://user:pass@api-v2.soundcloud.com/search",
        "https://api-v2.soundcloud.com:444/search",
        "not a URL",
    ] {
        assert!(build_api_url(raw, &[], "client").is_err(), "URL: {raw}");
    }
}

#[test]
fn api_url_errors_do_not_expose_raw_urls_or_secret_query_values() {
    let secret = "VERY_SECRET_CLIENT_QUERY_VALUE";
    let raw = format!("https://evil.com/path?client_id={secret}");
    let error = build_api_url(&raw, &[], secret).unwrap_err();

    assert!(!error.contains(&raw));
    assert!(!error.contains(secret));
}
