use super::probe_args;

#[test]
fn probe_args_include_cookies_only_for_youtube_hosts() {
    let cookies = vec!["--cookies".to_string(), "C:/private/cookies.txt".to_string()];
    for url in [
        "https://youtube.com/watch?v=1",
        "https://www.youtube.com/watch?v=1",
        "https://music.youtube.com/watch?v=1",
        "https://youtu.be/abc",
        "https://m.youtu.be/abc",
    ] {
        let args = probe_args(url, &cookies).expect("YouTube URL should be accepted");
        assert!(args.windows(2).any(|window| window == cookies), "URL: {url}");
    }
}

#[test]
fn probe_args_omit_cookies_for_non_youtube_or_lookalike_hosts() {
    let cookies = vec!["--cookies".to_string(), "C:/private/cookies.txt".to_string()];
    for url in [
        "https://artist.bandcamp.com/track/song",
        "https://soundcloud.com/artist/song",
        "https://evil.com/?next=https://youtube.com/watch?v=1",
        "https://youtube.com.evil.com/watch?v=1",
        "https://notyoutube.com/watch?v=1",
    ] {
        let args = probe_args(url, &cookies).expect("valid remote URL should be accepted");
        assert!(!args.contains(&"--cookies".to_string()), "URL: {url}");
        assert!(!args.contains(&"C:/private/cookies.txt".to_string()), "URL: {url}");
    }
}
