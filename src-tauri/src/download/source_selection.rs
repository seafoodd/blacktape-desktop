fn local_tier_rank(tier: crate::types::QualityTier) -> u8 {
    match tier {
        crate::types::QualityTier::Low => 0,
        crate::types::QualityTier::Standard => 1,
        crate::types::QualityTier::High => 2,
        crate::types::QualityTier::Lossless => 3,
    }
}

fn candidate_tier_rank(tier: crate::metadata::types::AudioQualityTier) -> u8 {
    match tier {
        crate::metadata::types::AudioQualityTier::Low => 0,
        crate::metadata::types::AudioQualityTier::StandardLossy => 1,
        crate::metadata::types::AudioQualityTier::HighLossy => 2,
        crate::metadata::types::AudioQualityTier::Lossless => 3,
    }
}

/// True when downloading `cand_tier` would produce strictly better quality than `local_tier`,
/// or when the local audio file is missing on disk. Pure.
pub fn candidate_improves_local(
    local_tier: crate::types::QualityTier,
    local_file_exists: bool,
    cand_tier: crate::metadata::types::AudioQualityTier,
) -> bool {
    !local_file_exists || candidate_tier_rank(cand_tier) > local_tier_rank(local_tier)
}

fn best_candidate_index(candidates: &[crate::metadata::types::AudioCandidate]) -> usize {
    let mut best_idx = 0;
    let mut best_key = (0u8, 0u32, false, 0i64);
    for (idx, c) in candidates.iter().enumerate() {
        let key = (
            candidate_tier_rank(c.quality_tier),
            c.bitrate_kbps.unwrap_or(0),
            c.is_strict_match,
            (c.match_confidence * 1000.0).round() as i64,
        );
        if idx == 0 || key > best_key {
            best_idx = idx;
            best_key = key;
        }
    }
    best_idx
}

/// Selects the matching candidate index along with its primary URL and fallback URLs. Pure.
pub fn pick_candidate_entry_for_tier(
    candidates: &[crate::metadata::types::AudioCandidate],
    preferred_format: Option<&str>,
    selected_idx: usize,
) -> Option<(usize, String, Vec<String>)> {
    use crate::metadata::types::AudioQualityTier;
    if candidates.is_empty() {
        return None;
    }

    let fmt = preferred_format.map(str::trim).unwrap_or("");
    let chosen_pos = if let Some(source_spec) = fmt.strip_prefix("source:") {
        let mut parts = source_spec.splitn(3, ':');
        let target_plat = parts.next().unwrap_or("").to_ascii_lowercase();
        let target_tier = parts.next().unwrap_or("").to_ascii_lowercase();
        let target_hash = parts.next().filter(|s| !s.is_empty());
        let matches_plat_tier = |c: &crate::metadata::types::AudioCandidate| {
            let plat_ok = c.source.platform_name().eq_ignore_ascii_case(&target_plat);
            if !plat_ok {
                return false;
            }
            match target_tier.as_str() {
                "standard" => c.quality_tier == AudioQualityTier::StandardLossy,
                "high" => c.quality_tier == AudioQualityTier::HighLossy,
                "lossless" => c.quality_tier == AudioQualityTier::Lossless,
                _ => true,
            }
        };
        target_hash
            .and_then(|hash| {
                candidates.iter().position(|c| match &c.source {
                    crate::metadata::types::AudioSource::Torrent { info_hash, .. } => {
                        matches_plat_tier(c) && info_hash.eq_ignore_ascii_case(hash)
                    }
                    _ => false,
                })
            })
            .or_else(|| candidates.iter().position(matches_plat_tier))
            .or_else(|| {
                candidates
                    .iter()
                    .position(|c| c.source.platform_name().eq_ignore_ascii_case(&target_plat))
            })
            .unwrap_or(0)
    } else if fmt.eq_ignore_ascii_case("lossless") {
        candidates
            .iter()
            .position(|c| c.quality_tier == AudioQualityTier::Lossless)
            .unwrap_or_else(|| best_candidate_index(candidates))
    } else if fmt.eq_ignore_ascii_case("standard") {
        candidates
            .iter()
            .position(|c| c.quality_tier == AudioQualityTier::StandardLossy)
            .or_else(|| candidates.iter().position(|c| c.quality_tier == AudioQualityTier::Low))
            .unwrap_or(0)
    } else if fmt.eq_ignore_ascii_case("high") {
        candidates
            .iter()
            .position(|c| c.quality_tier == AudioQualityTier::HighLossy)
            .unwrap_or(0)
    } else if selected_idx < candidates.len() {
        selected_idx
    } else {
        0
    };

    let primary = candidates[chosen_pos].source.primary_url().to_string();
    let mut fallbacks = Vec::new();
    for (idx, c) in candidates.iter().enumerate() {
        if idx == chosen_pos {
            continue;
        }
        let u = c.source.primary_url().to_string();
        if !u.is_empty() && u != primary && !fallbacks.contains(&u) {
            fallbacks.push(u);
        }
    }
    Some((chosen_pos, primary, fallbacks))
}

#[cfg(test)]
fn pick_candidate_urls_for_tier(
    candidates: &[crate::metadata::types::AudioCandidate],
    preferred_format: Option<&str>,
    selected_idx: usize,
) -> (String, Vec<String>) {
    match pick_candidate_entry_for_tier(candidates, preferred_format, selected_idx) {
        Some((_, primary, fallbacks)) => (primary, fallbacks),
        None => (String::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_candidate_urls_respects_tier_and_source_specs() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
        let cand = |plat: &str, url: &str, tier: AudioQualityTier| AudioCandidate {
            source: AudioSource::HttpStream {
                platform: plat.into(),
                url: url.into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: tier,
            codec: "Test".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: plat.into(),
            matched_title: Some("Song".into()),
            matched_artist: Some("Artist".into()),
        };
        let list = vec![
            cand("bandcamp", "https://bc.bandcamp.com/track/1", AudioQualityTier::Lossless),
            cand("youtube", "https://youtube.com/watch?v=1", AudioQualityTier::HighLossy),
            cand("soundcloud", "https://soundcloud.com/a/1", AudioQualityTier::StandardLossy),
        ];

        assert_eq!(pick_candidate_urls_for_tier(&list, Some("best"), 0).0, "https://bc.bandcamp.com/track/1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("high"), 0).0, "https://youtube.com/watch?v=1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("standard"), 0).0, "https://soundcloud.com/a/1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("source:youtube"), 0).0, "https://youtube.com/watch?v=1");
        assert_eq!(pick_candidate_urls_for_tier(&list, Some("best"), 1).0, "https://youtube.com/watch?v=1");
    }

    #[test]
    fn candidate_improves_local_checks_file_existence_and_strict_tier_increase() {
        use crate::metadata::types::AudioQualityTier;
        use crate::types::QualityTier;

        // Missing local file should always be re-downloadable regardless of recorded tier.
        assert!(candidate_improves_local(QualityTier::Lossless, false, AudioQualityTier::StandardLossy));

        // Same or lower tier when local file exists should skip upgrade.
        assert!(!candidate_improves_local(QualityTier::High, true, AudioQualityTier::HighLossy));
        assert!(!candidate_improves_local(QualityTier::High, true, AudioQualityTier::StandardLossy));
        assert!(!candidate_improves_local(QualityTier::Lossless, true, AudioQualityTier::HighLossy));
        assert!(!candidate_improves_local(QualityTier::Lossless, true, AudioQualityTier::Lossless));

        // Strictly higher tier improves the local file.
        assert!(candidate_improves_local(QualityTier::Low, true, AudioQualityTier::StandardLossy));
        assert!(candidate_improves_local(QualityTier::Standard, true, AudioQualityTier::HighLossy));
        assert!(candidate_improves_local(QualityTier::High, true, AudioQualityTier::Lossless));
    }

    #[test]
    fn pick_candidate_entry_for_tier_returns_chosen_index_alongside_urls() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let cand = |plat: &str, url: &str, tier: AudioQualityTier| AudioCandidate {
            source: AudioSource::HttpStream {
                platform: plat.into(),
                url: url.into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: tier,
            codec: "Test".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: plat.into(),
            matched_title: Some("Song".into()),
            matched_artist: Some("Artist".into()),
        };
        let list = vec![
            cand("bandcamp", "https://bc.bandcamp.com/track/1", AudioQualityTier::Lossless),
            cand("youtube", "https://youtube.com/watch?v=1", AudioQualityTier::HighLossy),
        ];

        let (idx_best, primary_best, fallbacks_best) =
            pick_candidate_entry_for_tier(&list, Some("best"), 0).unwrap();
        assert_eq!(idx_best, 0);
        assert_eq!(primary_best, "https://bc.bandcamp.com/track/1");
        assert_eq!(fallbacks_best, vec!["https://youtube.com/watch?v=1"]);

        let (idx_high, primary_high, _) =
            pick_candidate_entry_for_tier(&list, Some("high"), 0).unwrap();
        assert_eq!(idx_high, 1);
        assert_eq!(primary_high, "https://youtube.com/watch?v=1");
    }

    #[test]
    fn pick_candidate_entry_for_tier_selects_specific_torrent_info_hash_when_given() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let tor = |hash: &str| AudioCandidate {
            source: AudioSource::Torrent {
                info_hash: hash.into(),
                magnet_uri: format!("magnet:?xt=urn:btih:{hash}"),
                file_index: 0,
                file_path: "01.flac".into(),
                file_size: 1000,
                seeders: 10,
            },
            quality_tier: AudioQualityTier::Lossless,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: "TORRENT".into(),
            matched_title: None,
            matched_artist: None,
        };
        let list = vec![tor("archive-hash"), tor("rutracker-hash")];
        let (idx, _, _) =
            pick_candidate_entry_for_tier(&list, Some("source:torrent:lossless:rutracker-hash"), 0)
                .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn pick_candidate_entry_for_tier_lossless_ignores_stale_youtube_selected_idx() {
        use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};

        let youtube = AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "youtube".into(),
                url: "https://youtube.com/watch?v=1".into(),
                direct_stream_url: None,
                format_id: None,
                expires_at: None,
            },
            quality_tier: AudioQualityTier::HighLossy,
            codec: "OPUS".into(),
            bitrate_kbps: Some(160),
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: "YOUTUBE".into(),
            matched_title: None,
            matched_artist: None,
        };
        let flac_torrent = AudioCandidate {
            source: AudioSource::Torrent {
                info_hash: "archive-hash".into(),
                magnet_uri: "magnet:?xt=urn:btih:archive-hash".into(),
                file_index: 0,
                file_path: "01.flac".into(),
                file_size: 1000,
                seeders: 1,
            },
            quality_tier: AudioQualityTier::Lossless,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            sample_rate_hz: None,
            duration_ms: 180_000,
            match_confidence: 1.0,
            is_strict_match: true,
            display_label: "TORRENT".into(),
            matched_title: None,
            matched_artist: None,
        };

        // When lossless is requested, it picks Lossless even if selected_idx points to YouTube.
        let list = vec![youtube, flac_torrent];
        let (idx, primary, fallbacks) =
            pick_candidate_entry_for_tier(&list, Some("lossless"), 0).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(primary, "magnet:?xt=urn:btih:archive-hash");
        assert_eq!(fallbacks, vec!["https://youtube.com/watch?v=1"]);
    }

}
