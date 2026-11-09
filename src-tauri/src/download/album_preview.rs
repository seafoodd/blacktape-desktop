//! Album preview: the track list of an album/set of any platform, so the user can pick tracks before
//! downloading. Built on the same parsers the download uses (`parse_album`), so what is listed is what gets
//! downloaded (`process_album_url` can download a subset of these tracks by url).

use super::soundcloud::SkippedTrack;
use super::{bandcamp, soundcloud, youtube, AlbumDownload};
use crate::types::Platform;
use serde::{Deserialize, Serialize};

/// One row of the album preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumTrackInfo {
    /// Position in the album (0-based, also for unavailable rows).
    pub index: usize,
    pub title: String,
    pub artists: Vec<String>,
    pub track_number: Option<i32>,
    /// Page url of the track; the download selects tracks by it. Empty for unavailable tracks.
    pub url: String,
    /// False for tracks that cannot be downloaded (e.g. SoundCloud Go+ previews).
    pub available: bool,
    /// Why the track is unavailable.
    pub note: Option<String>,
    /// Length in ms when the platform provides it.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlbumPreview {
    pub platform: Platform,
    pub url: String,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub tracks: Vec<AlbumTrackInfo>,
    /// Sum of the known track durations in ms; None when no track has a duration.
    pub total_duration_ms: Option<u64>,
}

/// Maps a parsed album (plus the tracks a platform had to skip) to the preview. Pure.
///
/// `skipped` tracks keep their place: SoundCloud reports the 1-based position they had in the set, so they
/// are inserted there as unavailable rows.
pub fn build_preview(
    platform: Platform,
    url: &str,
    album: AlbumDownload,
    skipped: &[SkippedTrack],
) -> AlbumPreview {
    let mut rows: Vec<AlbumTrackInfo> = album
        .tracks
        .iter()
        .map(|t| AlbumTrackInfo {
            index: 0,
            title: t.title.clone(),
            artists: t.artists.clone(),
            track_number: t.track_number,
            url: t.url.clone(),
            available: true,
            note: None,
            duration_ms: t.duration_ms,
        })
        .collect();

    let mut skipped_sorted: Vec<&SkippedTrack> = skipped.iter().collect();
    skipped_sorted.sort_by_key(|s| s.position);
    for s in skipped_sorted {
        let at = s.position.saturating_sub(1).min(rows.len());
        rows.insert(
            at,
            AlbumTrackInfo {
                index: 0,
                title: s.title.clone(),
                artists: Vec::new(),
                track_number: i32::try_from(s.position).ok(),
                url: String::new(),
                available: false,
                note: Some(s.reason.clone()),
                duration_ms: None,
            },
        );
    }
    for (i, row) in rows.iter_mut().enumerate() {
        row.index = i;
        if row.track_number.is_none() {
            row.track_number = i32::try_from(i + 1).ok();
        }
    }

    let total_duration_ms = total_duration(&rows);
    AlbumPreview {
        platform,
        url: url.to_string(),
        title: album.title,
        artist: album.album_artist,
        cover_url: album.external_cover_url,
        year: album.release_year,
        genres: album.genres.unwrap_or_default(),
        tracks: rows,
        total_duration_ms,
    }
}

/// Sum of the known durations; None when none is known. Pure.
pub fn total_duration(rows: &[AlbumTrackInfo]) -> Option<u64> {
    let known: Vec<u64> = rows.iter().filter_map(|r| r.duration_ms).collect();
    if known.is_empty() {
        None
    } else {
        Some(known.iter().sum())
    }
}

/// Fetches the preview of the album/set at `url`.
pub async fn album_preview(platform: Platform, url: &str) -> Result<AlbumPreview, String> {
    let (album, skipped) = match platform {
        Platform::Bandcamp => (bandcamp::parse_album(url).await?, Vec::new()),
        Platform::Youtube => (youtube::parse_album(url).await?, Vec::new()),
        Platform::Soundcloud => soundcloud::parse_album_with_skipped(url).await?,
        Platform::Local => return Err("Local albums have no preview.".to_string()),
    };
    Ok(build_preview(platform, url, album, &skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::TrackDownload;

    fn track(title: &str, url: &str, number: Option<i32>) -> TrackDownload {
        TrackDownload {
            url: url.to_string(),
            title: title.to_string(),
            artists: vec!["Artist".to_string()],
            track_number: number,
            ..TrackDownload::default()
        }
    }

    fn album(tracks: Vec<TrackDownload>) -> AlbumDownload {
        AlbumDownload {
            title: "Album".to_string(),
            artists: vec!["Artist".to_string()],
            album_artist: "Artist".to_string(),
            tracks,
            genres: Some(vec!["Ambient".to_string()]),
            release_year: Some(2020),
            external_cover_url: Some("https://img/c.jpg".to_string()),
        }
    }

    #[test]
    fn maps_album_fields_and_indexes_tracks() {
        let preview = build_preview(
            Platform::Bandcamp,
            "https://a.bandcamp.com/album/x",
            album(vec![track("One", "https://u/1", Some(1)), track("Two", "https://u/2", None)]),
            &[],
        );
        assert_eq!(preview.title, "Album");
        assert_eq!(preview.artist, "Artist");
        assert_eq!(preview.year, Some(2020));
        assert_eq!(preview.genres, vec!["Ambient".to_string()]);
        assert_eq!(preview.cover_url.as_deref(), Some("https://img/c.jpg"));
        assert_eq!(preview.tracks.len(), 2);
        assert_eq!(preview.tracks[0].index, 0);
        assert_eq!(preview.tracks[1].index, 1);
        assert_eq!(preview.tracks[1].track_number, Some(2), "missing numbers fall back to the position");
        assert!(preview.tracks.iter().all(|t| t.available && t.note.is_none()));
    }

    #[test]
    fn skipped_tracks_keep_their_place_as_unavailable_rows() {
        let skipped = vec![SkippedTrack {
            position: 2,
            title: "Go+ track".to_string(),
            reason: "only a 30-second preview".to_string(),
        }];
        let preview = build_preview(
            Platform::Soundcloud,
            "https://soundcloud.com/a/sets/b",
            album(vec![track("One", "https://u/1", Some(1)), track("Three", "https://u/3", Some(3))]),
            &skipped,
        );
        let titles: Vec<&str> = preview.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["One", "Go+ track", "Three"]);
        let row = &preview.tracks[1];
        assert!(!row.available);
        assert!(row.url.is_empty());
        assert_eq!(row.note.as_deref(), Some("only a 30-second preview"));
        assert_eq!(preview.tracks[2].index, 2);
    }

    #[test]
    fn skipped_position_beyond_the_end_is_appended() {
        let skipped = vec![SkippedTrack { position: 9, title: "x".to_string(), reason: "r".to_string() }];
        let preview = build_preview(Platform::Soundcloud, "u", album(vec![track("One", "https://u/1", Some(1))]), &skipped);
        assert_eq!(preview.tracks.len(), 2);
        assert!(!preview.tracks[1].available);
    }

    #[test]
    fn serializes_snake_case() {
        let preview = build_preview(Platform::Youtube, "u", album(vec![track("One", "https://u/1", Some(1))]), &[]);
        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(json["platform"], "youtube");
        assert!(json["tracks"][0].get("track_number").is_some());
        assert!(json.get("cover_url").is_some());
    }

    fn timed(title: &str, url: &str, ms: Option<u64>) -> TrackDownload {
        TrackDownload { duration_ms: ms, ..track(title, url, None) }
    }

    #[test]
    fn rows_copy_durations_and_the_total_sums_the_known_ones() {
        let preview = build_preview(
            Platform::Bandcamp,
            "u",
            album(vec![
                timed("One", "https://u/1", Some(180_000)),
                timed("Two", "https://u/2", None),
                timed("Three", "https://u/3", Some(61_500)),
            ]),
            &[],
        );
        let durations: Vec<Option<u64>> = preview.tracks.iter().map(|t| t.duration_ms).collect();
        assert_eq!(durations, vec![Some(180_000), None, Some(61_500)]);
        assert_eq!(preview.total_duration_ms, Some(241_500), "gaps are skipped, not counted as 0");
    }

    #[test]
    fn total_is_none_when_no_track_has_a_duration() {
        let preview = build_preview(
            Platform::Youtube,
            "u",
            album(vec![timed("One", "https://u/1", None), timed("Two", "https://u/2", None)]),
            &[],
        );
        assert_eq!(preview.total_duration_ms, None);
        assert_eq!(total_duration(&[]), None);
    }

    #[test]
    fn skipped_rows_have_no_duration_and_do_not_change_the_total() {
        let skipped = vec![SkippedTrack { position: 1, title: "Go+".to_string(), reason: "preview".to_string() }];
        let preview = build_preview(
            Platform::Soundcloud,
            "u",
            album(vec![timed("Two", "https://u/2", Some(100_000))]),
            &skipped,
        );
        assert_eq!(preview.tracks[0].duration_ms, None);
        assert_eq!(preview.tracks[1].duration_ms, Some(100_000));
        assert_eq!(preview.total_duration_ms, Some(100_000));
    }

    #[test]
    fn total_duration_sums_rows_directly() {
        let row = |ms| AlbumTrackInfo {
            index: 0,
            title: String::new(),
            artists: vec![],
            track_number: None,
            url: String::new(),
            available: true,
            note: None,
            duration_ms: ms,
        };
        assert_eq!(total_duration(&[row(Some(1)), row(None), row(Some(2))]), Some(3));
    }

    #[test]
    fn serialization_has_both_duration_fields_and_nulls_for_unknown() {
        let preview = build_preview(
            Platform::Bandcamp,
            "u",
            album(vec![timed("One", "https://u/1", Some(90_000)), timed("Two", "https://u/2", None)]),
            &[],
        );
        let json = serde_json::to_value(&preview).unwrap();
        assert_eq!(json["total_duration_ms"], 90_000);
        assert_eq!(json["tracks"][0]["duration_ms"], 90_000);
        assert!(json["tracks"][1]["duration_ms"].is_null());

        let empty = build_preview(Platform::Bandcamp, "u", album(vec![track("One", "https://u/1", None)]), &[]);
        let json = serde_json::to_value(&empty).unwrap();
        assert!(json.get("total_duration_ms").is_some_and(|v| v.is_null()));
    }

    #[test]
    fn track_info_deserializes_without_a_duration() {
        let row: AlbumTrackInfo = serde_json::from_str(
            r#"{"index":0,"title":"t","artists":[],"track_number":1,"url":"u","available":true,"note":null}"#,
        )
        .unwrap();
        assert_eq!(row.duration_ms, None);
    }

    #[test]
    fn local_albums_are_rejected() {
        let result = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(album_preview(Platform::Local, "x"));
        assert!(result.is_err());
    }

    /// Live check (network): `cargo test --lib download::album_preview::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_bandcamp_album_preview() {
        let preview = album_preview(
            Platform::Bandcamp,
            "https://devon-wells.bandcamp.com/album/daft-punk-remixes-2012",
        )
        .await
        .expect("preview");
        println!(
            "{} - {} ({:?}) cover={:?} tracks={}",
            preview.artist, preview.title, preview.year, preview.cover_url.is_some(), preview.tracks.len()
        );
        for t in &preview.tracks {
            println!("  {:?} {} -> {}", t.track_number, t.title, t.url);
        }
        assert!(!preview.tracks.is_empty());
        assert!(preview.tracks.iter().all(|t| t.available && t.url.starts_with("https://")));
    }
}
