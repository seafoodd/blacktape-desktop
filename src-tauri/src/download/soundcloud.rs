//! SoundCloud download metadata: turns a track or set URL into [`TrackDownload`]s.
//! yt-dlp downloads the audio, as for the other platforms.
//!
//! Tracks that can't be downloaded in full are never queued. A single track fails with a
//! clear reason. A set skips those tracks and reports them, and fails only when nothing
//! in it can be downloaded.

use crate::download::{AlbumDownload, TrackDownload};
use crate::soundcloud::{
    self, artist_and_title, availability, playlist_artwork, track_artwork, track_download_url,
    year_from_dates, ScPlaylist, ScResource, ScTrack,
};
use crate::utils::sanitize_fs;

/// A track of a set that was left out, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedTrack {
    pub position: usize,
    pub title: String,
    pub reason: String,
}

pub fn describe_skipped(skipped: &[SkippedTrack]) -> String {
    let list = skipped
        .iter()
        .map(|s| format!("#{} {} ({})", s.position, s.title, s.reason))
        .collect::<Vec<_>>()
        .join("; ");
    format!("Skipped {} SoundCloud track(s): {list}", skipped.len())
}

pub async fn parse_track(url: &str) -> Result<(TrackDownload, Option<String>), String> {
    match soundcloud::resolve(url).await? {
        ScResource::Track(track) => {
            let cover = track_artwork(&track);
            let download = track_to_download(&track, None, None)?;
            Ok((download, cover))
        }
        ScResource::Playlist(_) => {
            Err("This SoundCloud link is a set; download it as an album instead.".into())
        }
        ScResource::User(_) => {
            Err("This SoundCloud link is a profile, not a track.".into())
        }
        ScResource::Other => Err("Unsupported SoundCloud link.".into()),
    }
}

pub async fn parse_album(url: &str) -> Result<AlbumDownload, String> {
    parse_album_with_skipped(url).await.map(|(album, _)| album)
}

pub async fn parse_album_with_skipped(
    url: &str,
) -> Result<(AlbumDownload, Vec<SkippedTrack>), String> {
    let mut playlist = match soundcloud::resolve(url).await? {
        ScResource::Playlist(p) => p,
        ScResource::Track(_) => {
            return Err("This SoundCloud link is a single track, not a set.".into())
        }
        ScResource::User(_) => {
            return Err("Downloading a whole SoundCloud profile isn't supported; open one of its sets.".into())
        }
        ScResource::Other => return Err("Unsupported SoundCloud link.".into()),
    };
    soundcloud::hydrate_playlist_tracks(&mut playlist).await?;
    let (album, skipped) = playlist_to_album(&playlist)?;
    if !skipped.is_empty() {
        eprintln!("[SoundCloud] {}", describe_skipped(&skipped));
    }
    Ok((album, skipped))
}

/// Album context shared by the tracks of a set.
pub struct AlbumContext<'a> {
    pub title: &'a str,
    pub album_artist: &'a str,
    pub genre: Option<&'a str>,
    pub release_year: Option<i32>,
}

pub fn track_to_download(
    track: &ScTrack,
    album: Option<&AlbumContext>,
    track_number: Option<i32>,
) -> Result<TrackDownload, String> {
    if let Some(reason) = availability(track).reason() {
        let title = track.title.clone().unwrap_or_else(|| format!("track {}", track.id));
        return Err(format!("Can't download \"{title}\" from SoundCloud: {reason}."));
    }
    let url = track_download_url(track).ok_or("SoundCloud track has no permalink URL")?;
    let (artist, title) = artist_and_title(track);

    let (album_title, album_artist, genre, release_year) = match album {
        Some(ctx) => (
            ctx.title.to_string(),
            ctx.album_artist.to_string(),
            non_empty(track.genre.as_deref()).or(ctx.genre),
            year_from_dates(&[track.release_date.as_deref()]).or(ctx.release_year),
        ),
        None => (
            track
                .publisher_metadata
                .as_ref()
                .and_then(|p| non_empty(p.album_title.as_deref()))
                .unwrap_or("Singles")
                .to_string(),
            artist.clone(),
            non_empty(track.genre.as_deref()),
            year_from_dates(&[track.release_date.as_deref(), track.display_date.as_deref()]),
        ),
    };

    Ok(TrackDownload {
        file_name: format!("{}.mp3", sanitize_fs(&title)),
        title,
        artists: vec![artist],
        album_artist,
        album: album_title,
        track_number,
        genres: genre.map(|g| vec![g.to_string()]),
        release_year,
        url,
        source_item_id: Some(track.id.to_string()),
        duration_ms: track_duration_ms(track),
    })
}

/// Builds the album from a set whose stubs were already hydrated. Track numbers keep the
/// set position, so skipping a preview leaves a gap instead of renumbering the rest.
pub fn playlist_to_album(
    playlist: &ScPlaylist,
) -> Result<(AlbumDownload, Vec<SkippedTrack>), String> {
    let album_artist = playlist
        .user
        .as_ref()
        .map(|u| u.username.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "Unknown Artist".into());
    let title = if playlist.title.trim().is_empty() {
        "Unknown Album".to_string()
    } else {
        playlist.title.trim().to_string()
    };
    let release_year = year_from_dates(&[
        playlist.release_date.as_deref(),
        playlist.published_at.as_deref(),
        playlist.display_date.as_deref(),
    ]);
    let genre = non_empty(playlist.genre.as_deref());
    let ctx = AlbumContext {
        title: &title,
        album_artist: &album_artist,
        genre,
        release_year,
    };

    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    for (idx, track) in playlist.tracks.iter().enumerate() {
        let position = idx + 1;
        match track_to_download(track, Some(&ctx), Some(position as i32)) {
            Ok(t) => tracks.push(t),
            Err(_) => skipped.push(SkippedTrack {
                position,
                title: track
                    .title
                    .clone()
                    .unwrap_or_else(|| format!("track {}", track.id)),
                reason: availability(track).reason().unwrap_or_default(),
            }),
        }
    }

    if tracks.is_empty() {
        return Err(if playlist.tracks.is_empty() {
            "This SoundCloud set is empty.".to_string()
        } else {
            format!(
                "None of the {} tracks in this SoundCloud set can be downloaded. {}",
                playlist.tracks.len(),
                describe_skipped(&skipped)
            )
        });
    }

    let genres = genre
        .map(|g| vec![g.to_string()])
        .or_else(|| tracks.iter().find_map(|t| t.genres.clone()));

    Ok((
        AlbumDownload {
            title,
            artists: vec![album_artist.clone()],
            album_artist,
            tracks,
            genres,
            release_year,
            external_cover_url: playlist_artwork(playlist),
        },
        skipped,
    ))
}

/// Length of the full track in ms: `full_duration` is the real one (`duration` is 30 s for Go+ previews),
/// `duration` is the fallback. Zero means unknown.
pub fn track_duration_ms(track: &ScTrack) -> Option<u64> {
    track
        .full_duration
        .filter(|d| *d > 0)
        .or(track.duration.filter(|d| *d > 0))
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/soundcloud/playlist_royal_concept_ep.json"
    ));
    const TRACK: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/soundcloud/track_flickermood.json"
    ));

    fn playlist() -> ScPlaylist {
        serde_json::from_str(PLAYLIST).unwrap()
    }
    fn track() -> ScTrack {
        serde_json::from_str(TRACK).unwrap()
    }

    #[test]
    fn single_track_maps_metadata() {
        let t = track_to_download(&track(), None, None).unwrap();
        assert_eq!(t.title, "Flickermood");
        assert_eq!(t.artists, vec!["Forss".to_string()]);
        assert_eq!(t.album_artist, "Forss");
        assert_eq!(t.album, "Singles");
        assert_eq!(t.url, "https://soundcloud.com/forss/flickermood");
        assert_eq!(t.genres, Some(vec!["Electronic".to_string()]));
        assert_eq!(t.release_year, Some(2003));
        assert_eq!(t.source_item_id.as_deref(), Some("293"));
        assert_eq!(t.track_number, None);
    }

    #[test]
    fn single_track_has_its_full_duration() {
        let t = track_to_download(&track(), None, None).unwrap();
        assert_eq!(t.duration_ms, Some(213_890), "full_duration wins over duration (213886)");
    }

    #[test]
    fn duration_falls_back_to_duration_and_zero_is_unknown() {
        let mut t = track();
        t.full_duration = None;
        assert_eq!(track_duration_ms(&t), Some(213_886));
        t.full_duration = Some(0);
        assert_eq!(track_duration_ms(&t), Some(213_886));
        t.duration = Some(0);
        assert_eq!(track_duration_ms(&t), None);
        t.duration = None;
        t.full_duration = None;
        assert_eq!(track_duration_ms(&t), None);
        t.full_duration = Some(200_000);
        assert_eq!(track_duration_ms(&t), Some(200_000));
    }

    #[test]
    fn set_tracks_carry_their_durations() {
        let (album, _) = playlist_to_album(&playlist()).unwrap();
        let durations: Vec<Option<u64>> = album.tracks.iter().map(|t| t.duration_ms).collect();
        assert_eq!(durations, vec![Some(205_348), Some(219_747)]);
    }

    #[test]
    fn preview_track_is_refused_with_reason() {
        let p = playlist();
        let snipped = &p.tracks[0];
        let err = track_to_download(snipped, None, None).unwrap_err();
        assert!(err.contains("30-second preview"), "{err}");
        assert!(err.contains("World On Fire"), "{err}");
    }

    #[test]
    fn set_skips_previews_and_unloaded_stubs_and_keeps_positions() {
        let (album, skipped) = playlist_to_album(&playlist()).unwrap();
        assert_eq!(album.title, "The Royal Concept EP");
        assert_eq!(album.album_artist, "The Royal Concept");
        assert_eq!(album.genres, Some(vec!["Indie/pop".to_string()]));
        assert_eq!(album.release_year, Some(2012));
        assert_eq!(
            album.external_cover_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-000030896212-o16m9v-t500x500.jpg")
        );

        let kept: Vec<(Option<i32>, &str)> = album
            .tracks
            .iter()
            .map(|t| (t.track_number, t.title.as_str()))
            .collect();
        assert_eq!(kept, vec![(Some(2), "Gimme Twice"), (Some(4), "D-D-Dance")]);
        assert!(album
            .tracks
            .iter()
            .all(|t| t.album == "The Royal Concept EP" && t.album_artist == "The Royal Concept"));

        let positions: Vec<usize> = skipped.iter().map(|s| s.position).collect();
        assert_eq!(positions, vec![1, 3, 5, 6]);
        assert!(skipped[0].reason.contains("preview"));
        assert!(skipped[3].reason.contains("metadata"));
        assert!(describe_skipped(&skipped).starts_with("Skipped 4 SoundCloud track(s): #1"));
    }

    #[test]
    fn hydrated_stub_is_kept() {
        let mut p = playlist();
        let mut full = track();
        full.id = p.tracks[5].id;
        soundcloud::merge_hydrated_tracks(&mut p.tracks, vec![full]);
        let (album, skipped) = playlist_to_album(&p).unwrap();
        assert_eq!(album.tracks.len(), 3);
        assert_eq!(album.tracks[2].track_number, Some(6));
        assert_eq!(album.tracks[2].title, "Flickermood");
        // A track's own genre wins over the set's.
        assert_eq!(album.tracks[2].genres, Some(vec!["Electronic".to_string()]));
        assert_eq!(skipped.len(), 3);
    }

    #[test]
    fn set_with_only_previews_is_an_error() {
        let mut p = playlist();
        p.tracks.retain(|t| t.policy.as_deref() == Some("SNIP"));
        let err = playlist_to_album(&p).unwrap_err();
        assert!(err.starts_with("None of the 3 tracks"), "{err}");
    }

    #[test]
    fn empty_set_is_an_error() {
        let mut p = playlist();
        p.tracks.clear();
        assert_eq!(playlist_to_album(&p).unwrap_err(), "This SoundCloud set is empty.");
    }
}
