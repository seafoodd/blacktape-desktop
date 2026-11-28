use crate::metadata::types::AudioCandidate;
use std::cmp::Ordering;

/// Sorts audio candidates from best to worst based on:
/// 1. Strict match flag (strict matches always outrank non-strict matches)
/// 2. Quality tier (Lossless > HighLossy > StandardLossy > Low)
/// 3. Bitrate (higher is better)
/// 4. Match confidence score
pub fn rank_candidates(candidates: &mut [AudioCandidate]) {
    candidates.sort_by(|a, b| {
        // 1. Strict match comparison
        match (a.is_strict_match, b.is_strict_match) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }

        // 2. Quality tier comparison (Lossless is greatest in enum Ord)
        match b.quality_tier.cmp(&a.quality_tier) {
            Ordering::Equal => {}
            other => return other,
        }

        // 3. Bitrate comparison
        let bitrate_a = a.bitrate_kbps.unwrap_or(0);
        let bitrate_b = b.bitrate_kbps.unwrap_or(0);
        match bitrate_b.cmp(&bitrate_a) {
            Ordering::Equal => {}
            other => return other,
        }

        // 4. Match confidence comparison
        b.match_confidence
            .partial_cmp(&a.match_confidence)
            .unwrap_or(Ordering::Equal)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::types::{AudioQualityTier, AudioSource};

    fn make_test_candidate(
        label: &str,
        tier: AudioQualityTier,
        bitrate: Option<u32>,
        confidence: f32,
        strict: bool,
    ) -> AudioCandidate {
        AudioCandidate {
            source: AudioSource::HttpStream {
                platform: "test".into(),
                url: "https://test.com/stream".into(),
                format_id: None,
                direct_stream_url: None,
                expires_at: None,
            },
            quality_tier: tier,
            codec: "Opus".into(),
            bitrate_kbps: bitrate,
            sample_rate_hz: Some(44100),
            duration_ms: 240000,
            match_confidence: confidence,
            is_strict_match: strict,
            display_label: label.into(),
            matched_title: Some(label.into()),
            matched_artist: None,
        }
    }

    #[test]
    fn ranks_strict_lossless_above_high_lossy() {
        let mut list = vec![
            make_test_candidate("YouTube Opus 160", AudioQualityTier::HighLossy, Some(160), 0.95, true),
            make_test_candidate("Bandcamp FLAC", AudioQualityTier::Lossless, Some(900), 0.95, true),
            make_test_candidate("SoundCloud MP3 128", AudioQualityTier::StandardLossy, Some(128), 0.90, true),
        ];

        rank_candidates(&mut list);

        assert_eq!(list[0].display_label, "Bandcamp FLAC");
        assert_eq!(list[1].display_label, "YouTube Opus 160");
        assert_eq!(list[2].display_label, "SoundCloud MP3 128");
    }

    #[test]
    fn ranks_strict_match_above_non_strict_even_if_higher_tier() {
        // If a candidate is lossless but NOT a strict match (e.g. wrong duration/remix),
        // a strict match HighLossy track should be preferred!
        let mut list = vec![
            make_test_candidate("Non-strict FLAC", AudioQualityTier::Lossless, Some(900), 0.60, false),
            make_test_candidate("Strict Opus 160", AudioQualityTier::HighLossy, Some(160), 0.95, true),
        ];

        rank_candidates(&mut list);

        assert_eq!(list[0].display_label, "Strict Opus 160");
        assert_eq!(list[1].display_label, "Non-strict FLAC");
    }

    #[test]
    fn ranks_higher_bitrate_within_same_tier() {
        let mut list = vec![
            make_test_candidate("Opus 160", AudioQualityTier::HighLossy, Some(160), 0.95, true),
            make_test_candidate("AAC 256", AudioQualityTier::HighLossy, Some(256), 0.95, true),
            make_test_candidate("MP3 320", AudioQualityTier::HighLossy, Some(320), 0.95, true),
        ];

        rank_candidates(&mut list);

        assert_eq!(list[0].display_label, "MP3 320");
        assert_eq!(list[1].display_label, "AAC 256");
        assert_eq!(list[2].display_label, "Opus 160");
    }
}
