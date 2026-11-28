use super::{retain_failed_cursors, FullSearchPage, PlatformWarning, SearchCursor};
use crate::{search::progress::Source, types::Platform};

fn warning(platform: Platform) -> PlatformWarning {
    PlatformWarning { platform, source: None, message: "request timed out".into() }
}

#[test]
fn failed_youtube_keeps_its_old_cursor_while_soundcloud_advances() {
    let previous = SearchCursor { youtube: Some("yt-old".into()), soundcloud: Some(40) };
    let mut page = FullSearchPage {
        next_cursor: Some(SearchCursor { youtube: None, soundcloud: Some(60) }),
        has_more: true,
        warnings: vec![warning(Platform::Youtube)],
        ..Default::default()
    };

    retain_failed_cursors(&mut page, &previous);

    assert_eq!(page.next_cursor, Some(SearchCursor { youtube: Some("yt-old".into()), soundcloud: Some(60) }));
    assert!(page.has_more);
}

#[test]
fn failed_soundcloud_keeps_its_old_offset_while_youtube_advances() {
    let previous = SearchCursor { youtube: Some("yt-old".into()), soundcloud: Some(40) };
    let mut page = FullSearchPage {
        next_cursor: Some(SearchCursor { youtube: Some("yt-new".into()), soundcloud: None }),
        has_more: true,
        warnings: vec![warning(Platform::Soundcloud)],
        ..Default::default()
    };

    retain_failed_cursors(&mut page, &previous);

    assert_eq!(page.next_cursor, Some(SearchCursor { youtube: Some("yt-new".into()), soundcloud: Some(40) }));
    assert!(page.has_more);
}

#[test]
fn successful_exhausted_sources_are_not_resurrected() {
    let previous = SearchCursor { youtube: Some("yt-old".into()), soundcloud: Some(40) };
    let mut page = FullSearchPage { next_cursor: None, has_more: false, ..Default::default() };

    retain_failed_cursors(&mut page, &previous);

    assert_eq!(page.next_cursor, None);
    assert!(!page.has_more);
}

#[test]
fn musicbrainz_warning_does_not_mark_a_platform_cursor_failed() {
    let previous = SearchCursor { youtube: Some("yt-old".into()), soundcloud: Some(40) };
    let mut page = FullSearchPage {
        next_cursor: None,
        warnings: vec![PlatformWarning {
            platform: Platform::Local,
            source: Some(Source::Musicbrainz),
            message: "MusicBrainz unavailable".into(),
        }],
        ..Default::default()
    };

    retain_failed_cursors(&mut page, &previous);

    assert_eq!(page.next_cursor, None);
    assert!(!page.has_more);
}
