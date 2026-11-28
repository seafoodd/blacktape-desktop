use super::{search_dropdown, search_page};
use crate::{search::{full::SearchKind, search_youtube_with_fallback}, types::Platform};
use std::mem::size_of_val;

const MAX_FUTURE_BYTES: usize = 4 * 1024;

#[test]
fn platform_dispatch_futures_stay_within_tokio_stack_budget() {
    for platform in [Platform::Youtube, Platform::Bandcamp, Platform::Soundcloud] {
        let dropdown = search_dropdown(platform, "future size fixture");
        let dropdown_size = size_of_val(&dropdown);
        eprintln!("{platform:?} dropdown future: {dropdown_size} bytes");
        assert!(
            dropdown_size <= MAX_FUTURE_BYTES,
            "{platform:?} dropdown future is {dropdown_size} bytes; limit is {MAX_FUTURE_BYTES}"
        );

        let page = search_page(platform, "future size fixture", SearchKind::Track, None);
        let page_size = size_of_val(&page);
        eprintln!("{platform:?} page future: {page_size} bytes");
        assert!(
            page_size <= MAX_FUTURE_BYTES,
            "{platform:?} page future is {page_size} bytes; limit is {MAX_FUTURE_BYTES}"
        );
    }

    let youtube_fallback = search_youtube_with_fallback("future size fixture");
    let youtube_fallback_size = size_of_val(&youtube_fallback);
    eprintln!("YouTube fast/legacy fallback future: {youtube_fallback_size} bytes");
    assert!(
        youtube_fallback_size <= MAX_FUTURE_BYTES,
        "YouTube fast/legacy fallback future is {youtube_fallback_size} bytes; limit is {MAX_FUTURE_BYTES}"
    );
}
