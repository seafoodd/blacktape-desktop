use super::inspect;
use super::release::{InspectedRelease, InspectedTrack, ReleaseLayout};
use super::search::{self, SearchHit};
use crate::metadata::types::{AudioCandidate, AudioQualityTier, AudioSource};
use crate::resolver::matcher::{
    clean_title, has_unwanted_derivative, strip_leading_artist_from_title,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use strsim::jaro_winkler;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ExpectedTrackInput {
    pub index: usize,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub track_number: Option<u32>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentTierEstimate {
    pub tier: AudioQualityTier,
    pub codec: String,
    pub bitrate_kbps: Option<u32>,
    pub label: String,
    pub release_title: String,
    pub provider: String,
    pub seeders: Option<u32>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedTorrentTrack {
    pub expected_index: usize,
    pub candidate: AudioCandidate,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentTierResolution {
    pub tier: AudioQualityTier,
    pub info_hash: String,
    pub magnet_uri: String,
    pub release_title: String,
    pub provider: String,
    pub seeders: Option<u32>,
    pub codec: String,
    pub bitrate_kbps: Option<u32>,
    pub display_label: String,
    pub matched_count: usize,
    pub total_expected: usize,
    pub total_matched_bytes: u64,
    pub missing_titles: Vec<String>,
    pub ambiguous_titles: Vec<String>,
    pub layout_warning: Option<String>,
    pub tracks: Vec<MatchedTorrentTrack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TorrentResolveEvent {
    Searching,
    SearchProgress { checked: usize, total: usize },
    InspectionProgress { checked: usize, total: usize },
    Estimated {
        estimates: Vec<TorrentTierEstimate>,
    },
    Verified {
        resolutions: Vec<TorrentTierResolution>,
    },
    Resolved {
        resolutions: Vec<TorrentTierResolution>,
        unavailable_providers: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTitleQuality {
    pub tier: AudioQualityTier,
    pub codec: &'static str,
    pub bitrate_kbps: Option<u32>,
    pub label: String,
}

/// Estimates audio quality from tokens in a release/torrent title before inspecting file headers. Pure.
pub fn estimate_quality_from_release_title(title: &str) -> Option<ReleaseTitleQuality> {
    let lower = title.to_ascii_lowercase();
    let tokens: Vec<String> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    if tokens.is_empty() {
        return None;
    }

    let has = |word: &str| tokens.iter().any(|t| t == word);

    if has("flac") || has("alac") || has("wav") || has("aiff") || has("ape") || has("wavpack") || has("wv") || has("lossless") || has("24bit") || has("hires") {
        let codec = if has("alac") {
            "ALAC"
        } else if has("wav") {
            "WAV"
        } else if has("aiff") {
            "AIFF"
        } else if has("ape") {
            "APE"
        } else if has("wavpack") || has("wv") {
            "WavPack"
        } else {
            "FLAC"
        };
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::Lossless,
            codec,
            bitrate_kbps: None,
            label: format!("{codec} (Lossless)"),
        });
    }

    if has("opus") {
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::HighLossy,
            codec: "Opus",
            bitrate_kbps: Some(160),
            label: "Opus (High)".into(),
        });
    }

    if has("320") || has("320kbps") || has("cbr320") || lower.contains("320 kbps") || lower.contains("320kbps") {
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::HighLossy,
            codec: "MP3",
            bitrate_kbps: Some(320),
            label: "MP3 320 kbps".into(),
        });
    }

    if has("v0") || has("vbr0") {
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::HighLossy,
            codec: "MP3",
            bitrate_kbps: Some(245),
            label: "MP3 V0".into(),
        });
    }

    if has("256") || has("256kbps") {
        let codec = if has("aac") || has("m4a") { "AAC" } else { "MP3" };
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::HighLossy,
            codec,
            bitrate_kbps: Some(256),
            label: format!("{codec} 256 kbps"),
        });
    }

    if has("192") || has("192kbps") || has("v2") || has("160") || has("128") || has("128kbps") {
        let bitrate = if has("192") || has("192kbps") || has("v2") {
            192
        } else if has("160") {
            160
        } else {
            128
        };
        let codec = if has("aac") || has("m4a") {
            "AAC"
        } else if has("ogg") {
            "Ogg"
        } else {
            "MP3"
        };
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::StandardLossy,
            codec,
            bitrate_kbps: Some(bitrate),
            label: format!("{codec} {bitrate} kbps"),
        });
    }

    if has("aac") || has("m4a") {
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::HighLossy,
            codec: "AAC",
            bitrate_kbps: Some(256),
            label: "AAC (High)".into(),
        });
    }

    if has("mp3") || has("ogg") {
        let codec = if has("ogg") { "Ogg" } else { "MP3" };
        return Some(ReleaseTitleQuality {
            tier: AudioQualityTier::StandardLossy,
            codec,
            bitrate_kbps: None,
            label: codec.into(),
        });
    }

    None
}

fn snap_mp3_bitrate(raw_kbps: u32) -> u32 {
    if raw_kbps >= 295 {
        320
    } else if raw_kbps >= 235 {
        256
    } else if raw_kbps >= 210 {
        224
    } else if raw_kbps >= 175 {
        192
    } else if raw_kbps >= 145 {
        160
    } else {
        128
    }
}

/// Computes quality tier, normalized codec, estimated bitrate, and label from inspected file bytes & expected duration.
pub fn classify_inspected_track_quality(
    codec: &str,
    size_bytes: u64,
    duration_ms: Option<u64>,
    title_hint: Option<&ReleaseTitleQuality>,
) -> (AudioQualityTier, String, Option<u32>, String) {
    let upper = codec.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "FLAC" | "ALAC" | "WAV" | "AIFF" | "APE" | "WAVPACK"
    ) {
        return (
            AudioQualityTier::Lossless,
            codec.to_string(),
            None,
            format!("{codec} (Lossless)"),
        );
    }

    let measured_kbps = duration_ms
        .filter(|&d| d >= 10_000 && size_bytes > 0)
        .map(|d| ((size_bytes.saturating_mul(8)) / d).clamp(32, 2000) as u32);

    if upper == "M4A" {
        if let Some(kbps) = measured_kbps {
            if kbps >= 500 {
                return (
                    AudioQualityTier::Lossless,
                    "ALAC".into(),
                    None,
                    "ALAC (Lossless)".into(),
                );
            }
            let tier = if kbps >= 220 {
                AudioQualityTier::HighLossy
            } else if kbps >= 110 {
                AudioQualityTier::StandardLossy
            } else {
                AudioQualityTier::Low
            };
            return (tier, "AAC".into(), Some(kbps), format!("AAC {kbps} kbps"));
        }
        if let Some(hint) = title_hint.filter(|h| h.tier == AudioQualityTier::Lossless) {
            return (
                AudioQualityTier::Lossless,
                hint.codec.into(),
                None,
                format!("{} (Lossless)", hint.codec),
            );
        }
        return (
            AudioQualityTier::HighLossy,
            "AAC".into(),
            title_hint.and_then(|h| h.bitrate_kbps).or(Some(256)),
            "AAC (High)".into(),
        );
    }

    let hinted_kbps = title_hint
        .filter(|h| h.codec.eq_ignore_ascii_case(codec))
        .and_then(|h| h.bitrate_kbps);

    let bitrate_kbps = match measured_kbps {
        Some(raw) if upper == "MP3" => Some(snap_mp3_bitrate(raw)),
        Some(raw) => Some(raw),
        None => hinted_kbps,
    };

    let tier = if let Some(kbps) = bitrate_kbps {
        if upper == "OPUS" {
            if kbps >= 140 {
                AudioQualityTier::HighLossy
            } else if kbps >= 96 {
                AudioQualityTier::StandardLossy
            } else {
                AudioQualityTier::Low
            }
        } else if kbps >= 220 {
            AudioQualityTier::HighLossy
        } else if kbps >= 110 {
            AudioQualityTier::StandardLossy
        } else {
            AudioQualityTier::Low
        }
    } else if let Some(hint) = title_hint.filter(|h| h.tier != AudioQualityTier::Lossless) {
        hint.tier
    } else {
        AudioQualityTier::StandardLossy
    };

    let label = match bitrate_kbps {
        Some(kbps) => format!("{codec} {kbps} kbps"),
        None if tier == AudioQualityTier::HighLossy => format!("{codec} (High)"),
        None if tier == AudioQualityTier::StandardLossy => format!("{codec} (Standard)"),
        None => codec.to_string(),
    };

    (tier, codec.to_string(), bitrate_kbps, label)
}

/// True when the search hit is healthy enough to consider (has reported seeders >= 1 or comes from Archive.org).
pub fn hit_is_healthy(hit: &SearchHit) -> bool {
    match hit.seeders {
        Some(0) => false,
        Some(_) => true,
        None => hit.provider == "Archive.org",
    }
}

fn hit_quality_bucket(hit: &SearchHit) -> Option<AudioQualityTier> {
    estimate_quality_from_release_title(&hit.title).map(|q| q.tier)
}

fn codec_quality_rank(codec: &str) -> u8 {
    match codec.to_ascii_uppercase().as_str() {
        "FLAC" | "ALAC" | "WAV" | "AIFF" | "APE" | "WAVPACK" => 3,
        "OPUS" | "AAC" | "M4A" => 2,
        _ => 1,
    }
}

fn parenthetical_base(title: &str) -> Option<String> {
    let (base, _) = title.trim_end().strip_suffix(')')?.rsplit_once('(')?;
    let base = clean_title(base);
    (!base.is_empty()).then_some(base)
}

fn tagged_album_file(
    file: &InspectedTrack,
    normalized_artist: &str,
    normalized_release: &str,
) -> Option<InspectedTrack> {
    use crate::search::ranking::normalize;
    if normalized_artist.is_empty() {
        return None;
    }
    let (prefix, rest) = file.title.split_once(" - ")?;
    let (album, numbered_title) = rest.split_once(" - ")?;
    if normalize(prefix) != normalized_artist {
        return None;
    }
    let album = normalize(album);
    let prefix = format!("{normalized_artist} {album}");
    if album.is_empty() || !normalized_release.strip_prefix(&prefix)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
    {
        return None;
    }
    let (disc, track, title) = super::release::track_title(&[], numbered_title);
    Some(InspectedTrack {
        title: title.into(),
        track_number: Some(track?),
        disc_number: disc.or(file.disc_number),
        ..file.clone()
    })
}

fn score_file_match(
    expected: &ExpectedTrackInput,
    file: &InspectedTrack,
    default_artist: Option<&str>,
    version_base: Option<&str>,
) -> Option<(f32, bool)> {
    let artist = expected
        .artist
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(default_artist)
        .unwrap_or("");

    let stripped_file = strip_leading_artist_from_title(artist, &file.title);
    let target_clean = clean_title(&expected.title);
    let file_clean = clean_title(&stripped_file);

    if target_clean.is_empty() || file_clean.is_empty() {
        return None;
    }

    // A joined hidden track cannot substitute for either of its standalone files, or vice versa.
    if target_clean != file_clean
        && (expected.title.contains(" / ") || stripped_file.contains(" / "))
    {
        return None;
    }

    // A bare filename cannot identify which of several separately listed versions it contains.
    if target_clean != file_clean && version_base.is_some_and(|base|
        file_clean == base || parenthetical_base(&stripped_file).as_deref() == Some(base)
    ) {
        return None;
    }

    if has_unwanted_derivative(&expected.title, &stripped_file)
        || has_unwanted_derivative(&stripped_file, &expected.title)
    {
        return None;
    }

    // The released recording has two titles across storefronts. The recalled medley
    // has a different title and must stay separate.
    let released_title_alias = matches!(
        (target_clean.as_str(), file_clean.as_str()),
        ("not just what i needed", "not what i needed") | ("not what i needed", "not just what i needed")
    ) && crate::search::ranking::normalize(artist) == "car seat headrest";
    let title_sim = if target_clean == file_clean || released_title_alias {
        1.0
    } else {
        jaro_winkler(&target_clean, &file_clean) as f32
    };

    if title_sim < 0.84 {
        return None;
    }

    // Disambiguate multi-disc / identical track names via disc & track numbers when both are known.
    let disc_match = match (expected.disc_number, file.disc_number) {
        (Some(ed), Some(fd)) => ed == fd,
        _ => true,
    };
    let track_match = match (expected.track_number, file.track_number) {
        (Some(et), Some(ft)) => et == ft,
        _ => true,
    };

    // If titles are identical and track numbers conflict on the same disc, skip wrong slot unless needed.
    let mut score = title_sim;
    if expected.disc_number.is_some() && file.disc_number == expected.disc_number {
        score += 0.08;
    } else if !disc_match {
        score -= 0.15;
    }
    if expected.track_number.is_some() && file.track_number == expected.track_number {
        score += 0.06;
    } else if !track_match {
        score -= 0.05;
    }

    let strict = title_sim >= 0.94 && disc_match;
    Some((score, strict))
}

/// Maps an inspected release's audio files to the expected track structure (MusicBrainz/platform/local library). Pure.
pub fn match_release_to_expected(
    release: &InspectedRelease,
    hit: &SearchHit,
    expected: &[ExpectedTrackInput],
) -> TorrentTierResolution {
    let title_hint = estimate_quality_from_release_title(&hit.title)
        .or_else(|| estimate_quality_from_release_title(&release.name));
    let seeders_u32 = hit.seeders.and_then(|s| u32::try_from(s).ok());
    let default_artist = expected
        .iter()
        .find_map(|e| e.artist.as_deref().filter(|s| !s.trim().is_empty()));

    if release.layout == ReleaseLayout::CueImageAlbum {
        let fallback_tier = title_hint
            .as_ref()
            .map(|h| h.tier)
            .unwrap_or(AudioQualityTier::Lossless);
        let fallback_codec = title_hint.as_ref().map(|h| h.codec).unwrap_or("FLAC");
        return TorrentTierResolution {
            tier: fallback_tier,
            info_hash: release.info_hash.clone(),
            magnet_uri: hit.input.clone(),
            release_title: hit.title.clone(),
            provider: hit.provider.clone(),
            seeders: seeders_u32,
            codec: fallback_codec.into(),
            bitrate_kbps: title_hint.as_ref().and_then(|h| h.bitrate_kbps),
            display_label: title_hint
                .as_ref()
                .map(|h| h.label.clone())
                .unwrap_or_else(|| format!("{fallback_codec} (Lossless)")),
            matched_count: 0,
            total_expected: expected.len(),
            total_matched_bytes: 0,
            missing_titles: expected.iter().map(|e| e.title.clone()).collect(),
            ambiguous_titles: Vec::new(),
            layout_warning: Some(
                "Single audio image with CUE sheet — individual tracks cannot be split cleanly yet."
                    .into(),
            ),
            tracks: Vec::new(),
        };
    }

    let artist = crate::search::ranking::normalize(default_artist.unwrap_or_default());
    let release_title = crate::search::ranking::normalize(&hit.title);
    let normalized_files: Vec<_> = release
        .tracks
        .iter()
        .filter(|t| !t.is_sample)
        .map(|file| tagged_album_file(file, &artist, &release_title)
            .map_or(std::borrow::Cow::Borrowed(file), std::borrow::Cow::Owned))
        .collect();
    let audio_files: Vec<_> = normalized_files.iter().map(|file| file.as_ref()).collect();

    let mut versions = HashMap::<String, HashSet<String>>::new();
    for track in expected {
        if let Some(base) = parenthetical_base(&track.title) {
            versions.entry(base).or_default().insert(clean_title(&track.title));
        }
    }

    // Score candidates per expected track.
    let mut ranked_per_expected: Vec<Vec<(usize, f32, bool)>> = Vec::with_capacity(expected.len());

    for exp in expected {
        let base = parenthetical_base(&exp.title);
        let version_base = base.as_deref().filter(|base| versions.get(*base).is_some_and(|titles| titles.len() > 1));
        let mut opts: Vec<(usize, f32, bool)> = audio_files
            .iter()
            .enumerate()
            .filter_map(|(file_pos, file)| {
                let (score, strict) = score_file_match(exp, file, default_artist, version_base)?;
                Some((file_pos, score, strict))
            })
            .collect();
        opts.sort_by(|a, b| {
            let diff = b.1 - a.1;
            if diff.abs() < 0.005 {
                codec_quality_rank(audio_files[b.0].codec)
                    .cmp(&codec_quality_rank(audio_files[a.0].codec))
                    .then_with(|| audio_files[b.0].size_bytes.cmp(&audio_files[a.0].size_bytes))
            } else {
                b.1.total_cmp(&a.1)
            }
        });
        ranked_per_expected.push(opts);
    }

    // Resolve claims: highest-scoring expected track (with lossless codec preference on ties) keeps each file.
    let mut claimed_files = HashSet::<usize>::new();
    let mut best_assignment: HashMap<usize, (usize, f32, bool)> = HashMap::new();
    let mut claim_list: Vec<(usize, usize, f32, bool)> = Vec::new();
    for (exp_pos, opts) in ranked_per_expected.iter().enumerate() {
        for &(file_pos, score, strict) in opts {
            claim_list.push((exp_pos, file_pos, score, strict));
        }
    }
    claim_list.sort_by(|a, b| {
        let diff = b.2 - a.2;
        if diff.abs() < 0.005 {
            codec_quality_rank(audio_files[b.1].codec)
                .cmp(&codec_quality_rank(audio_files[a.1].codec))
                .then_with(|| audio_files[b.1].size_bytes.cmp(&audio_files[a.1].size_bytes))
        } else {
            b.2.total_cmp(&a.2)
        }
    });
    for (exp_pos, file_pos, score, strict) in claim_list {
        if best_assignment.contains_key(&exp_pos) || claimed_files.contains(&file_pos) {
            continue;
        }
        claimed_files.insert(file_pos);
        best_assignment.insert(exp_pos, (file_pos, score, strict));
    }

    let mut matched_tracks = Vec::new();
    let mut missing_titles = Vec::new();
    let mut ambiguous_titles = Vec::new();
    let mut total_matched_bytes = 0u64;
    let mut tier_counts: BTreeMap<AudioQualityTier, usize> = BTreeMap::new();

    for (exp_pos, exp) in expected.iter().enumerate() {
        let opts = &ranked_per_expected[exp_pos];
        let is_ambiguous = opts.len() >= 2
            && (opts[0].1 - opts[1].1).abs() < 0.01
            && audio_files[opts[0].0].codec.eq_ignore_ascii_case(audio_files[opts[1].0].codec);
        if let Some(&(file_pos, raw_score, strict)) = best_assignment.get(&exp_pos) {
            let file = audio_files[file_pos];
            let (tier, codec, bitrate_kbps, label) = classify_inspected_track_quality(
                file.codec,
                file.size_bytes,
                exp.duration_ms,
                title_hint.as_ref(),
            );
            *tier_counts.entry(tier).or_insert(0) += 1;
            total_matched_bytes = total_matched_bytes.saturating_add(file.size_bytes);
            if is_ambiguous {
                ambiguous_titles.push(exp.title.clone());
            }

            matched_tracks.push(MatchedTorrentTrack {
                expected_index: exp.index,
                candidate: AudioCandidate {
                    source: AudioSource::Torrent {
                        info_hash: release.info_hash.clone(),
                        magnet_uri: hit.input.clone(),
                        file_index: file.file_index,
                        file_path: file.file_path.clone(),
                        file_size: file.size_bytes,
                        seeders: seeders_u32.unwrap_or(0),
                    },
                    quality_tier: tier,
                    codec,
                    bitrate_kbps,
                    sample_rate_hz: None,
                    duration_ms: exp.duration_ms.unwrap_or(0),
                    match_confidence: raw_score.clamp(0.0, 1.0),
                    is_strict_match: strict && !is_ambiguous,
                    display_label: format!("Torrent · {label}"),
                    matched_title: Some(file.title.clone()),
                    matched_artist: exp.artist.clone(),
                },
            });
        } else {
            missing_titles.push(exp.title.clone());
        }
    }

    if expected.is_empty() {
        for (pos, file) in audio_files.iter().enumerate() {
            let (tier, codec, bitrate_kbps, label) = classify_inspected_track_quality(
                file.codec,
                file.size_bytes,
                None,
                title_hint.as_ref(),
            );
            *tier_counts.entry(tier).or_insert(0) += 1;
            total_matched_bytes = total_matched_bytes.saturating_add(file.size_bytes);
            matched_tracks.push(MatchedTorrentTrack {
                expected_index: pos,
                candidate: AudioCandidate {
                    source: AudioSource::Torrent {
                        info_hash: release.info_hash.clone(),
                        magnet_uri: hit.input.clone(),
                        file_index: file.file_index,
                        file_path: file.file_path.clone(),
                        file_size: file.size_bytes,
                        seeders: seeders_u32.unwrap_or(0),
                    },
                    quality_tier: tier,
                    codec,
                    bitrate_kbps,
                    sample_rate_hz: None,
                    duration_ms: 0,
                    match_confidence: 0.9,
                    is_strict_match: true,
                    display_label: format!("Torrent · {label}"),
                    matched_title: Some(file.title.clone()),
                    matched_artist: None,
                },
            });
        }
    }

    let dominant_tier = tier_counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(tier, _)| tier)
        .or_else(|| title_hint.as_ref().map(|h| h.tier))
        .unwrap_or(AudioQualityTier::StandardLossy);

    let mut codec_counts = BTreeMap::new();
    let mut bitrates = Vec::new();
    for track in matched_tracks.iter().filter(|track| track.candidate.quality_tier == dominant_tier) {
        *codec_counts.entry(track.candidate.codec.clone()).or_insert(0) += 1;
        bitrates.extend(track.candidate.bitrate_kbps);
    }
    let dominant_codec = codec_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(codec, _)| codec)
        .or_else(|| title_hint.as_ref().map(|h| h.codec.to_string()))
        .unwrap_or_else(|| "Audio".into());

    let median_bitrate = if bitrates.is_empty() {
        title_hint.as_ref().and_then(|h| h.bitrate_kbps)
    } else {
        bitrates.sort_unstable();
        Some(bitrates[bitrates.len() / 2])
    };

    let display_label = match dominant_tier {
        AudioQualityTier::Lossless => format!("{dominant_codec} (Lossless)"),
        AudioQualityTier::HighLossy => match median_bitrate {
            Some(kbps) => format!("{dominant_codec} {kbps} kbps"),
            None => format!("{dominant_codec} (High)"),
        },
        AudioQualityTier::StandardLossy => match median_bitrate {
            Some(kbps) => format!("{dominant_codec} {kbps} kbps"),
            None => format!("{dominant_codec} (Standard)"),
        },
        AudioQualityTier::Low => format!("{dominant_codec} (Low)"),
    };

    let total_expected = if expected.is_empty() {
        matched_tracks.len()
    } else {
        expected.len()
    };
    let layout_warning = if matched_tracks.is_empty() {
        Some("Torrent files did not match the expected album tracks.".into())
    } else if !missing_titles.is_empty() {
        Some(format!(
            "Matched {}/{} tracks · {} missing (will fall back to streams when using Best)",
            matched_tracks.len(),
            total_expected,
            missing_titles.len()
        ))
    } else if !ambiguous_titles.is_empty() {
        Some(format!(
            "{} track match{} may need verification",
            ambiguous_titles.len(),
            if ambiguous_titles.len() == 1 { "" } else { "es" }
        ))
    } else {
        None
    };

    TorrentTierResolution {
        tier: dominant_tier,
        info_hash: release.info_hash.clone(),
        magnet_uri: hit.input.clone(),
        release_title: hit.title.clone(),
        provider: hit.provider.clone(),
        seeders: seeders_u32,
        codec: dominant_codec,
        bitrate_kbps: median_bitrate,
        display_label,
        matched_count: matched_tracks.len(),
        total_expected,
        total_matched_bytes,
        missing_titles,
        ambiguous_titles,
        layout_warning,
        tracks: matched_tracks,
    }
}

/// Builds fast initial estimates grouped by quality tier from search hits before metadata inspection. Pure.
pub fn build_tier_estimates(hits: &[SearchHit]) -> Vec<TorrentTierEstimate> {
    let mut best_by_tier: BTreeMap<AudioQualityTier, TorrentTierEstimate> = BTreeMap::new();
    for hit in preferred_hits(hits) {
        if hit.rank < 2 || !hit_is_healthy(hit) {
            continue;
        }
        let Some(q) = estimate_quality_from_release_title(&hit.title) else {
            continue;
        };
        best_by_tier.entry(q.tier).or_insert_with(|| TorrentTierEstimate {
            tier: q.tier,
            codec: q.codec.to_string(),
            bitrate_kbps: q.bitrate_kbps,
            label: q.label,
            release_title: hit.title.clone(),
            provider: hit.provider.clone(),
            seeders: hit.seeders.and_then(|s| u32::try_from(s).ok()),
            size_bytes: hit.size_bytes,
        });
    }
    best_by_tier.into_values().rev().collect()
}

fn estimate_event(hits: &[SearchHit]) -> Option<TorrentResolveEvent> {
    hits.iter()
        .any(|hit| hit.rank >= 2 && hit_is_healthy(hit))
        .then(|| TorrentResolveEvent::Estimated {
            estimates: build_tier_estimates(hits),
        })
}

// Keep the search ranking within each group, but try reported active swarms before
// sources whose peer availability is unknown (notably Internet Archive).
fn preferred_hits(hits: &[SearchHit]) -> Vec<&SearchHit> {
    let mut ordered: Vec<_> = hits.iter().collect();
    ordered.sort_by_key(|hit| !hit.seeders.is_some_and(|count| count > 0));
    ordered
}

type ResolveCacheKey = (String, String, Vec<ExpectedTrackInput>, Vec<search::Provider>);
type ResolveCacheValue = (Vec<TorrentTierResolution>, Vec<String>, std::time::Instant);

fn resolved_torrent_cache() -> &'static std::sync::Mutex<HashMap<ResolveCacheKey, ResolveCacheValue>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<ResolveCacheKey, ResolveCacheValue>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn should_keep_resolution(res: &TorrentTierResolution, expected_len: usize) -> bool {
    if res.matched_count == 0 {
        return res
            .layout_warning
            .as_deref()
            .is_some_and(|w| w.contains("CUE"));
    }
    if expected_len >= 4 && res.matched_count < 2 {
        return false;
    }
    true
}

fn verified_inferred_release(resolution: &TorrentTierResolution, expected: &[ExpectedTrackInput]) -> bool {
    // Repeated song names and fuzzy matches do not independently identify a release.
    let expected_titles: HashMap<_, _> = expected.iter().map(|track| (track.index, clean_title(&track.title))).collect();
    let matched: HashSet<_> = resolution.tracks.iter()
        .filter(|track| track.candidate.is_strict_match)
        .filter_map(|track| {
            let title = track.candidate.matched_title.as_deref()?;
            let expected = expected_titles.get(&track.expected_index)?;
            let title = strip_leading_artist_from_title(
                track.candidate.matched_artist.as_deref().unwrap_or_default(), title,
            );
            (!expected.is_empty() && *expected == clean_title(&title)).then_some(expected)
        })
        .collect();
    matched.len() >= 4 && matched.len() >= expected.len() - expected.len() / 4
}

/// Searches providers, streams initial per-tier estimates, inspects top healthy hits per tier,
/// and matches files against `expected`.
#[cfg(test)]
pub async fn resolve_album_torrents(
    artist: &str,
    title: &str,
    expected: &[ExpectedTrackInput],
    emit: impl Fn(TorrentResolveEvent),
) -> Result<(Vec<TorrentTierResolution>, Vec<String>), String> {
    resolve_with_indexers(None, artist, title, expected, &search::Provider::ALL, emit).await
}

pub(super) async fn resolve_with_indexers(
    app: Option<&tauri::AppHandle>,
    artist: &str,
    title: &str,
    expected: &[ExpectedTrackInput],
    enabled: &[search::Provider],
    emit: impl Fn(TorrentResolveEvent),
) -> Result<(Vec<TorrentTierResolution>, Vec<String>), String> {
    // Account-backed results depend on the live session, including after a sign-in or sign-out.
    let cache_key: Option<ResolveCacheKey> = (!enabled.contains(&search::Provider::RuTracker)).then(|| (
        clean_title(artist),
        clean_title(title),
        expected.to_vec(),
        enabled.to_vec(),
    ));
    let cached = cache_key.as_ref().and_then(|cache_key| {
        let guard = resolved_torrent_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.get(cache_key).cloned()
    });
    if let Some((resolutions, unavailable_providers, at)) = &cached {
        if at.elapsed() < std::time::Duration::from_secs(300) && unavailable_providers.is_empty() {
            emit(TorrentResolveEvent::Resolved {
                resolutions: resolutions.clone(),
                unavailable_providers: unavailable_providers.clone(),
            });
            return Ok((resolutions.clone(), unavailable_providers.clone()));
        }
    }

    emit(TorrentResolveEvent::Searching);
    if let Some((resolutions, _, _)) = cached.filter(|(_, _, at)| at.elapsed() < std::time::Duration::from_secs(900)) {
        if !resolutions.is_empty() {
            emit(TorrentResolveEvent::Verified { resolutions });
        }
    }
    let (reports, mut pending_reports) = tokio::sync::mpsc::unbounded_channel();
    let search = search::search_with_app(app, artist, title, expected.len() >= 4, enabled, move |provider| {
        let _ = reports.send(provider);
    });
    tokio::pin!(search);

    let mut search_done = false;
    let mut provider_count = 0;
    let mut hits = Vec::new();
    let mut unavailable_providers = Vec::new();
    let mut tasks = tokio::task::JoinSet::new();
    let mut started = HashSet::new();
    let mut attempts = BTreeMap::new();
    let mut inferred_attempts = 0;
    let mut checked = 0;
    let mut last_progress = None;
    let mut resolutions = Vec::new();
    let expected = std::sync::Arc::new(expected.to_vec());

    loop {
        // Leave capacity for candidates from indexers that are still searching.
        let concurrency = if search_done { 4 } else { 2 };
        let per_tier_budget = if search_done { 6 } else { 2 };
        let mut remaining = 0;
        let mut pending_attempts = attempts.clone();
        let mut pending_inferred = inferred_attempts;
        for hit in inspection_order(&hits) {
            if !hit_is_healthy(hit) || started.contains(&hit.input) {
                continue;
            }
            let bucket = hit_quality_bucket(hit);
            if hit.rank >= 2 {
                let count = pending_attempts.entry(bucket).or_insert(0);
                if *count >= per_tier_budget { continue; }
                *count += 1;
            } else if !search_done || hit.rank != 1 || pending_inferred >= 2 {
                continue;
            } else {
                pending_inferred += 1;
            }
            if tasks.len() >= concurrency {
                remaining += 1;
                continue;
            }
            if hit.rank >= 2 {
                *attempts.entry(bucket).or_insert(0) += 1;
            } else {
                inferred_attempts += 1;
            }
            started.insert(hit.input.clone());
            let hit = hit.clone();
            let expected = expected.clone();
            #[cfg(not(test))]
            let app = app.cloned();
            tasks.spawn(async move {
                #[cfg(not(test))]
                let release = inspect::inspect_with_app(app.as_ref(), &hit.input).await;
                #[cfg(test)]
                let release = inspect::inspect(&hit.input).await;
                let release = release.ok()?;
                if release.layout == ReleaseLayout::NonAudio { return None; }
                let resolution = match_release_to_expected(&release, &hit, &expected);
                let verified = if hit.rank == 1 {
                    verified_inferred_release(&resolution, &expected)
                } else {
                    should_keep_resolution(&resolution, expected.len())
                };
                verified.then_some(resolution)
            });
        }
        let total = checked + tasks.len() + remaining;
        if total > 0 && last_progress != Some((checked, total)) {
            last_progress = Some((checked, total));
            emit(TorrentResolveEvent::InspectionProgress { checked, total });
        }
        if search_done && tasks.is_empty() { break; }

        tokio::select! {
            biased;
            Some(provider) = pending_reports.recv(), if !search_done => {
                provider_count += 1;
                if started.is_empty() {
                    emit(TorrentResolveEvent::SearchProgress { checked: provider_count, total: enabled.len() });
                }
                hits.extend(provider.hits);
                hits = search::merge_hits(hits);
                if let Some(event) = estimate_event(&hits) {
                    emit(event);
                }
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                checked += 1;
                if let Some(Ok(Some(resolution))) = result {
                    resolutions.push(resolution);
                    emit(TorrentResolveEvent::Verified { resolutions: sorted_resolutions(resolutions.clone()) });
                }
            }
            result = &mut search, if !search_done => {
                let report = result?;
                hits = report.hits;
                unavailable_providers = report.providers.into_iter()
                    .filter(|provider| provider.error.is_some()).map(|provider| provider.provider).collect();
                search_done = true;
            }
        }
    }
    let resolutions = sorted_resolutions(resolutions);
    if let Some(cache_key) = cache_key {
        let mut guard = resolved_torrent_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.len() >= 32 {
            guard.clear();
        }
        guard.insert(
            cache_key,
            (
                resolutions.clone(),
                unavailable_providers.clone(),
                std::time::Instant::now(),
            ),
        );
    }

    emit(TorrentResolveEvent::Resolved {
        resolutions: resolutions.clone(),
        unavailable_providers: unavailable_providers.clone(),
    });

    Ok((resolutions, unavailable_providers))
}

fn inspection_order(hits: &[SearchHit]) -> Vec<&SearchHit> {
    let mut ordered = preferred_hits(hits);
    ordered.sort_by_key(|hit| (
        std::cmp::Reverse(hit.rank),
        hit.input.starts_with("magnet:") && !hit.input.contains("&xs="),
        hit_quality_bucket(hit) != Some(AudioQualityTier::Lossless),
    ));
    ordered
}

pub(super) fn sorted_resolutions(mut resolutions: Vec<TorrentTierResolution>) -> Vec<TorrentTierResolution> {
    resolutions.sort_by_cached_key(|release| {
        let web_seed = super::web_seed::archive_item(&release.magnet_uri)
            .and_then(|item| {
                let bytes = inspect::cached_torrent_bytes(&release.magnet_uri)?;
                super::web_seed::ArchiveSeed::parse(&bytes, &item, &release.info_hash).ok()
            }).is_some();
        (
            std::cmp::Reverse(release.tier),
            std::cmp::Reverse(release.matched_count),
            std::cmp::Reverse(web_seed),
            std::cmp::Reverse(release.seeders),
            release.info_hash.clone(),
            release.magnet_uri.clone(),
        )
    });
    let mut seen = HashSet::new();
    let mut counts = BTreeMap::new();
    resolutions.retain(|release| {
        let count = counts.entry(release.tier).or_insert(0);
        if *count >= 6 || !seen.insert((release.tier, release.info_hash.clone())) {
            return false;
        }
        *count += 1;
        true
    });
    resolutions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str, provider: &str, seeders: Option<u64>, rank: u8) -> SearchHit {
        SearchHit {
            id: title.into(),
            title: title.into(),
            input: format!("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"),
            provider: provider.into(),
            providers: vec![provider.into()],
            info_hash: Some("0123456789abcdef0123456789abcdef01234567".into()),
            seeders,
            size_bytes: Some(500_000_000),
            popularity: None,
            rank,
        }
    }

    #[test]
    fn estimate_quality_from_release_title_separates_lossless_and_high_mp3() {
        let flac = estimate_quality_from_release_title("Aphex Twin - Drukqs [FLAC Lossless]").unwrap();
        assert_eq!(flac.tier, AudioQualityTier::Lossless);
        assert_eq!(flac.codec, "FLAC");

        let mp3_320 = estimate_quality_from_release_title("Aphex Twin - Drukqs (2001) MP3 320kbps").unwrap();
        assert_eq!(mp3_320.tier, AudioQualityTier::HighLossy);
        assert_eq!(mp3_320.codec, "MP3");
        assert_eq!(mp3_320.bitrate_kbps, Some(320));

        let mp3_192 = estimate_quality_from_release_title("Aphex Twin - Drukqs MP3 192kbps").unwrap();
        assert_eq!(mp3_192.tier, AudioQualityTier::StandardLossy);
        assert_eq!(mp3_192.bitrate_kbps, Some(192));
    }

    #[test]
    fn build_tier_estimates_emits_separate_healthy_tiers_and_skips_dead_torrents() {
        let hits = vec![
            hit("Aphex Twin - Drukqs [FLAC]", "ApiBay", Some(0), 3),
            hit("Aphex Twin - Drukqs (2001) [FLAC]", "Archive.org", None, 3),
            hit("Aphex Twin - Drukqs (MP3 320)", "ApiBay", Some(8), 3),
        ];
        let estimates = build_tier_estimates(&hits);
        assert_eq!(estimates.len(), 2);
        assert_eq!(estimates[0].tier, AudioQualityTier::Lossless);
        assert_eq!(estimates[0].provider, "Archive.org");
        assert_eq!(estimates[1].tier, AudioQualityTier::HighLossy);
        assert_eq!(estimates[1].bitrate_kbps, Some(320));
    }

    #[test]
    fn seeded_release_is_preferred_to_archive_with_unknown_peers() {
        let hits = vec![
            hit("Aphex Twin - Drukqs [FLAC]", "Archive.org", None, 3),
            hit("Aphex Twin - Drukqs (2001) [FLAC]", "Knaben", Some(47), 2),
        ];
        let ordered = preferred_hits(&hits);
        assert_eq!(ordered[0].provider, "Knaben");
        assert_eq!(build_tier_estimates(&hits)[0].provider, "Knaben");
    }

    #[test]
    fn inspection_checks_exact_metadata_before_magnets_of_the_same_rank() {
        let mut magnet = hit("Artist - Album FLAC", "Knaben", Some(100), 3);
        magnet.input = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into();
        let mut exact = hit("Artist - Album FLAC", "RuTor", Some(2), 3);
        exact.input = format!("{}&xs=https%3A%2F%2Fd.rutor.info%2Fdownload%2F1", magnet.input);
        let mut inferred = exact.clone();
        inferred.rank = 1;
        let hits = [inferred, magnet, exact];
        let order = inspection_order(&hits);
        assert!(std::ptr::eq(order[0], &hits[2]));
        assert!(std::ptr::eq(order[1], &hits[1]));
        assert!(std::ptr::eq(order[2], &hits[0]));
    }

    #[tokio::test]
    #[ignore = "measures progressive discovery using live providers for Nevermind"]
    async fn live_nevermind_first_verified_lossless() {
        let tracks = [
            "Smells Like Teen Spirit", "In Bloom", "Come As You Are", "Breed", "Lithium",
            "Polly", "Territorial Pissings", "Drain You", "Lounge Act", "Stay Away",
            "On a Plain", "Something in the Way / Endless, Nameless",
        ];
        let expected: Vec<_> = tracks.iter().enumerate().map(|(index, title)| ExpectedTrackInput {
            index,
            title: (*title).into(),
            artist: Some("Nirvana".into()),
            disc_number: Some(1),
            track_number: Some(index as u32 + 1),
            duration_ms: None,
        }).collect();
        let started = std::time::Instant::now();
        let first = std::sync::OnceLock::new();
        let (resolutions, unavailable) = resolve_album_torrents("Nirvana", "Nevermind", &expected, |event| {
            if let TorrentResolveEvent::Verified { resolutions } = event {
                if let Some(release) = resolutions.iter().find(|r| r.tier == AudioQualityTier::Lossless && r.matched_count > 0) {
                    first.get_or_init(|| (started.elapsed(), release.matched_count));
                }
            }
        }).await.unwrap();
        println!("Nevermind: first verified FLAC={:?}, completed={:?}, unavailable={unavailable:?}", first.get(), started.elapsed());
        assert!(first.get().is_some(), "no verified FLAC was published during discovery");
        assert!(resolutions.iter().any(|r| r.tier == AudioQualityTier::Lossless && r.matched_count > 0));
    }

    #[test]
    fn eligible_untagged_torrent_reports_matching_before_inspection() {
        let archive = hit("Aphex Twin - Syro", "Archive.org", None, 3);
        assert!(matches!(
            estimate_event(&[archive]),
            Some(TorrentResolveEvent::Estimated { estimates }) if estimates.is_empty()
        ));
        let dead = hit("Aphex Twin - Syro [FLAC]", "Knaben", Some(0), 3);
        assert!(estimate_event(&[dead]).is_none());
    }

    #[test]
    fn classify_inspected_track_quality_detects_mp3_320_vs_alac_m4a() {
        let (mp3_tier, mp3_codec, mp3_kbps, _) =
            classify_inspected_track_quality("MP3", 7_200_000, Some(180_000), None);
        assert_eq!(mp3_tier, AudioQualityTier::HighLossy);
        assert_eq!(mp3_codec, "MP3");
        assert_eq!(mp3_kbps, Some(320));

        let (alac_tier, alac_codec, _, _) =
            classify_inspected_track_quality("M4A", 20_000_000, Some(180_000), None);
        assert_eq!(alac_tier, AudioQualityTier::Lossless);
        assert_eq!(alac_codec, "ALAC");
    }

    #[test]
    fn bare_deadlines_filename_does_not_match_either_digital_edition_version() {
        let expected = |title: &str| ExpectedTrackInput {
            index: 0, title: title.into(), artist: Some("Car Seat Headrest".into()),
            disc_number: Some(1), track_number: Some(3), duration_ms: None,
        };
        let mut file = InspectedTrack {
            file_index: 2, file_path: "03 Deadlines.mp3".into(), title: "Deadlines".into(),
            codec: "MP3", size_bytes: 5_000_000, disc_number: None, track_number: Some(3),
            folder: None, is_sample: false,
        };
        for title in ["Deadlines (Hostile)", "Deadlines (Thoughtful)"] {
            assert!(score_file_match(&expected(title), &file, None, Some("deadlines")).is_none());
        }
        assert!(score_file_match(&expected("Deadlines"), &file, None, None).unwrap().1);
        file.title = "Deadlines (Hostile)".into();
        assert!(score_file_match(&expected("Deadlines (Hostile)"), &file, None, Some("deadlines")).unwrap().1);
        assert!(score_file_match(&expected("Deadlines (Thoughtful)"), &file, None, Some("deadlines")).is_none());
    }

    #[test]
    fn nevermind_joined_hidden_track_requires_the_joined_file() {
        let joined = ExpectedTrackInput {
            index: 11, title: "Something in the Way / Endless, Nameless".into(),
            artist: Some("Nirvana".into()), disc_number: Some(1), track_number: Some(12),
            duration_ms: Some(1_235_000),
        };
        let file = InspectedTrack {
            file_index: 11, file_path: "12 Something in the Way.flac".into(),
            title: "Something in the Way".into(), codec: "FLAC", size_bytes: 30_000_000,
            disc_number: Some(1), track_number: Some(12), folder: None, is_sample: false,
        };
        assert!(score_file_match(&joined, &file, None, None).is_none());
        let exact = InspectedTrack { title: joined.title.clone(), ..file };
        assert!(score_file_match(&joined, &exact, None, None).unwrap().1);
    }

    #[test]
    fn album_tagged_filenames_require_matching_artist_album_and_track_number() {
        let file = InspectedTrack {
            file_index: 8,
            file_path: "Car Seat Headrest - 4 - 02 who even knows.flac".into(),
            title: "Car Seat Headrest - 4 - 02 who even knows".into(),
            codec: "FLAC",
            size_bytes: 23_000_000,
            disc_number: None,
            track_number: None,
            folder: None,
            is_sample: false,
        };
        let tagged = tagged_album_file(&file, "car seat headrest", "car seat headrest 4 flac").unwrap();
        assert_eq!(tagged.title, "who even knows");
        assert_eq!(tagged.track_number, Some(2));
        assert_eq!(tagged.file_path, file.file_path);
        assert!(tagged_album_file(&file, "another artist", "another artist 4").is_none());
        assert!(tagged_album_file(&file, "car seat headrest", "car seat headrest 43").is_none());
        let unnumbered = InspectedTrack { title: "Car Seat Headrest - 4 - who even knows".into(), ..file };
        assert!(tagged_album_file(&unnumbered, "car seat headrest", "car seat headrest 4").is_none());
    }

    #[test]
    fn denial_released_title_alias_is_exact_but_recalled_medley_is_separate() {
        let mut expected = ExpectedTrackInput {
            index: 4, title: "Not Just What I Needed".into(), artist: Some("Car Seat Headrest".into()),
            disc_number: Some(1), track_number: Some(5), duration_ms: Some(271_000),
        };
        let mut file = InspectedTrack {
            file_index: 4, file_path: "05 Not What I Needed.mp3".into(), title: "Not What I Needed".into(),
            codec: "MP3", size_bytes: 8_000_000, disc_number: None, track_number: Some(5),
            folder: None, is_sample: false,
        };
        assert!(score_file_match(&expected, &file, None, None).unwrap().1);
        expected.artist = Some("Another Artist".into());
        assert!(!score_file_match(&expected, &file, None, None).is_some_and(|(_, strict)| strict));
        expected.artist = Some("Car Seat Headrest".into());
        file.title = "Just What I Needed , Not Just What I Needed".into();
        file.codec = "FLAC";
        assert!(score_file_match(&expected, &file, None, None).is_none());
    }

    #[test]
    fn mixed_torrent_reports_a_codec_from_the_dominant_quality_tier() {
        let release = InspectedRelease {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            name: "Mixed".into(), layout: ReleaseLayout::MultiDiscAlbum,
            total_size_bytes: 80_000_000, total_file_count: 4,
            tracks: ["FLAC", "WAV", "MP3", "MP3"].into_iter().enumerate().map(|(index, codec)| InspectedTrack {
                file_index: index, file_path: format!("{index}.{codec}"), title: format!("Song {index}"),
                codec, size_bytes: 20_000_000, disc_number: None, track_number: None, folder: None, is_sample: false,
            }).collect(),
            cover_art: Vec::new(), cue_sheets: Vec::new(), rip_logs: Vec::new(),
        };
        let result = match_release_to_expected(&release, &hit("Artist - Mixed", "Archive.org", None, 3), &[]);
        assert_eq!(result.tier, AudioQualityTier::Lossless);
        assert!(matches!(result.codec.as_str(), "FLAC" | "WAV"));
    }

    #[test]
    fn match_release_to_expected_handles_multi_disc_and_reports_missing_tracks() {
        let release = InspectedRelease {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            name: "Drukqs".into(),
            layout: ReleaseLayout::MultiDiscAlbum,
            total_size_bytes: 900_000_000,
            total_file_count: 3,
            tracks: vec![
                InspectedTrack {
                    file_index: 1,
                    file_path: "CD1/01 Jynweythek.flac".into(),
                    title: "Jynweythek".into(),
                    codec: "FLAC",
                    size_bytes: 20_000_000,
                    disc_number: Some(1),
                    track_number: Some(1),
                    folder: Some("CD1".into()),
                    is_sample: false,
                },
                InspectedTrack {
                    file_index: 4,
                    file_path: "CD2/01 54 Cymru Beats.flac".into(),
                    title: "54 Cymru Beats".into(),
                    codec: "FLAC",
                    size_bytes: 45_000_000,
                    disc_number: Some(2),
                    track_number: Some(1),
                    folder: Some("CD2".into()),
                    is_sample: false,
                },
            ],
            cover_art: Vec::new(),
            cue_sheets: Vec::new(),
            rip_logs: Vec::new(),
        };
        let expected = vec![
            ExpectedTrackInput {
                index: 0,
                title: "Jynweythek".into(),
                artist: Some("Aphex Twin".into()),
                disc_number: Some(1),
                track_number: Some(1),
                duration_ms: Some(143_000),
            },
            ExpectedTrackInput {
                index: 15,
                title: "54 Cymru Beats".into(),
                artist: Some("Aphex Twin".into()),
                disc_number: Some(2),
                track_number: Some(1),
                duration_ms: Some(367_000),
            },
            ExpectedTrackInput {
                index: 16,
                title: "Btoum-Roumada".into(),
                artist: Some("Aphex Twin".into()),
                disc_number: Some(2),
                track_number: Some(2),
                duration_ms: Some(118_000),
            },
        ];

        let h = hit("Aphex Twin - Drukqs [FLAC]", "ApiBay", Some(12), 3);
        let res = match_release_to_expected(&release, &h, &expected);
        assert_eq!(res.tier, AudioQualityTier::Lossless);
        assert_eq!(res.matched_count, 2);
        assert_eq!(res.total_expected, 3);
        assert_eq!(res.missing_titles, vec!["Btoum-Roumada".to_string()]);
        assert_eq!(res.tracks[0].expected_index, 0);
        assert_eq!(res.tracks[1].expected_index, 15);
        assert_eq!(res.total_matched_bytes, 65_000_000);
        assert!(res.layout_warning.as_deref().unwrap().contains("2/3 tracks"));
    }

    #[test]
    fn match_release_prefers_lossless_over_archive_derivative_codecs_without_ambiguity() {
        let release = InspectedRelease {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            name: "Drukqs".into(),
            layout: ReleaseLayout::StandardAlbum,
            total_size_bytes: 50_000_000,
            total_file_count: 3,
            tracks: vec![
                InspectedTrack {
                    file_index: 0,
                    file_path: "01 Jynweythek.ogg".into(),
                    title: "Jynweythek".into(),
                    codec: "Ogg",
                    size_bytes: 3_000_000,
                    disc_number: None,
                    track_number: Some(1),
                    folder: None,
                    is_sample: false,
                },
                InspectedTrack {
                    file_index: 1,
                    file_path: "01 Jynweythek.mp3".into(),
                    title: "Jynweythek".into(),
                    codec: "MP3",
                    size_bytes: 4_000_000,
                    disc_number: None,
                    track_number: Some(1),
                    folder: None,
                    is_sample: false,
                },
                InspectedTrack {
                    file_index: 2,
                    file_path: "01 Jynweythek.flac".into(),
                    title: "Jynweythek".into(),
                    codec: "FLAC",
                    size_bytes: 20_000_000,
                    disc_number: None,
                    track_number: Some(1),
                    folder: None,
                    is_sample: false,
                },
            ],
            cover_art: Vec::new(),
            cue_sheets: Vec::new(),
            rip_logs: Vec::new(),
        };
        let expected = vec![ExpectedTrackInput {
            index: 0,
            title: "Jynweythek".into(),
            artist: Some("Aphex Twin".into()),
            disc_number: Some(1),
            track_number: Some(1),
            duration_ms: Some(143_000),
        }];
        let h = hit("Aphex Twin - Drukqs", "Archive.org", None, 3);
        let res = match_release_to_expected(&release, &h, &expected);
        assert_eq!(res.tier, AudioQualityTier::Lossless);
        assert_eq!(res.codec, "FLAC");
        assert!(res.ambiguous_titles.is_empty());
        assert_eq!(res.layout_warning, None);
        assert_eq!(res.tracks[0].candidate.codec, "FLAC");
    }

    #[test]
    fn should_keep_resolution_filters_non_cue_empty_and_single_track_scraps() {
        let base = TorrentTierResolution {
            tier: AudioQualityTier::Lossless,
            info_hash: "hash".into(),
            magnet_uri: "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into(),
            release_title: "Album".into(),
            provider: "Archive.org".into(),
            seeders: None,
            codec: "FLAC".into(),
            bitrate_kbps: None,
            display_label: "FLAC (Lossless)".into(),
            matched_count: 0,
            total_expected: 10,
            total_matched_bytes: 0,
            missing_titles: Vec::new(),
            ambiguous_titles: Vec::new(),
            layout_warning: Some("Torrent files did not match the expected album tracks.".into()),
            tracks: Vec::new(),
        };
        assert!(!should_keep_resolution(&base, 10));

        let cue = TorrentTierResolution {
            layout_warning: Some("Single audio image with CUE sheet — individual tracks cannot be split cleanly yet.".into()),
            ..base.clone()
        };
        assert!(should_keep_resolution(&cue, 10));

        let one_of_eight = TorrentTierResolution {
            matched_count: 1,
            total_expected: 8,
            layout_warning: None,
            ..base.clone()
        };
        assert!(!should_keep_resolution(&one_of_eight, 8));
        assert!(should_keep_resolution(&one_of_eight, 1));
    }

    #[test]
    fn weak_release_labels_require_four_distinct_exact_tracks_and_three_quarters_coverage() {
        let titles = ["Weightlifters", "Can’t Cool Me Down", "Hollywood", "Hymn (remix)", "Martin"];
        let expected: Vec<_> = titles.iter().enumerate().map(|(index, title)| ExpectedTrackInput {
            index, title: (*title).into(), artist: Some("Car Seat Headrest".into()),
            disc_number: None, track_number: Some(index as u32 + 1), duration_ms: None,
        }).collect();
        let files = |titles: &[&str]| titles.iter().enumerate().map(|(index, title)| InspectedTrack {
            file_index: index, file_path: format!("{title}.flac"), title: (*title).into(),
            codec: "FLAC", size_bytes: 20_000_000, disc_number: None,
            track_number: Some(index as u32 + 1), folder: None, is_sample: false,
        }).collect();
        let mut release = InspectedRelease {
            info_hash: "0123456789abcdef0123456789abcdef01234567".into(),
            name: "Making a Door Less Open".into(), layout: ReleaseLayout::StandardAlbum,
            total_size_bytes: 80_000_000, total_file_count: 4, tracks: files(&titles[..4]),
            cover_art: Vec::new(), cue_sheets: Vec::new(), rip_logs: Vec::new(),
        };
        let hit = hit("Making a Door Less Open [FLAC]", "Archive.org", None, 1);
        let matched = match_release_to_expected(&release, &hit, &expected);
        assert!(verified_inferred_release(&matched, &expected));
        release.tracks = files(&["Weightlifters", "Can’t Cool Me Down", "Hollywood", "Hymn (live)", "Hollywood"]);
        let wrong = match_release_to_expected(&release, &hit, &expected);
        assert!(!verified_inferred_release(&wrong, &expected));
        let mut longer = expected.clone();
        longer.push(ExpectedTrackInput { index: 5, title: "Famous".into(), ..expected[0].clone() });
        assert!(!verified_inferred_release(&matched, &longer));
    }

    #[tokio::test]
    #[ignore = "live network diagnostic across ~36 real albums and tracks"]
    async fn live_corpus_resolution_diagnostic() {
        let mb = crate::metadata::MusicBrainzClient::new(None);
        let cases: &[(&str, &str, bool)] = &[
            ("The Doors", "The Doors", true),
            ("The Doors", "The Soft Parade", true),
            ("Aphex Twin", "Drukqs", true),
            ("Fleetwood Mac", "Rumours", true),
            ("Radiohead", "In Rainbows", true),
            ("Daft Punk", "Discovery", true),
            ("Nirvana", "Nevermind", true),
            ("Kate Bush", "Hounds of Love", true),
            ("Miles Davis", "Kind of Blue", true),
            ("Kendrick Lamar", "To Pimp a Butterfly", true),
            ("Sade", "Diamond Life", true),
            ("Björk", "Vespertine", true),
            ("Madvillain", "Madvillainy", true),
            ("Pink Floyd", "The Wall", true),
            ("Outkast", "Speakerboxxx/The Love Below", true),
            ("Stevie Wonder", "Songs in the Key of Life", true),
            ("The Smashing Pumpkins", "Mellon Collie and the Infinite Sadness", true),
            ("The Rolling Stones", "Exile on Main St.", true),
            ("Bob Dylan", "Blonde on Blonde", true),
            ("Nine Inch Nails", "Ghosts I-IV", true),
            ("Led Zeppelin", "Physical Graffiti", true),
            ("David Bowie", "The Rise and Fall of Ziggy Stardust and the Spiders from Mars", true),
            ("Metallica", "Master of Puppets", true),
            ("The Cure", "Disintegration", true),
            ("R.E.M.", "Automatic for the People", true),
            ("Taylor Swift", "1989 (Taylor's Version)", true),
            ("Sweet Trip", "Velocity Design Comfort", true),
            ("The Books", "The Lemon of Pink", true),
            ("Boards of Canada", "Music Has the Right to Children", true),
            ("Portishead", "Dummy", true),
            // Track cases:
            ("The Doors", "Light My Fire", false),
            ("Radiohead", "Paranoid Android", false),
            ("Daft Punk", "Digital Love", false),
            ("Nirvana", "Come as You Are", false),
            ("Pink Floyd", "Wish You Were Here", false),
            ("Kendrick Lamar", "King Kunta", false),
        ];

        let mut full_match = 0usize;
        let mut partial_match = 0usize;
        let mut cue_only = 0usize;
        let mut zero_hits = 0usize;
        let mut inspect_failed_all = 0usize;

        for (i, &(artist, title, is_album)) in cases.iter().enumerate() {
            let mut expected = Vec::new();
            if is_album {
                if let Some(rg_id) = mb.find_matching_release_group_id(artist, title).await {
                    if let Ok(editions) = mb.list_grouped_editions_for_group(&rg_id).await {
                        let pick = editions.iter().find(|e| e.is_default).or(editions.first());
                        if let Some(ed) = pick {
                            if let Ok(canonical) = mb.fetch_canonical_release(&ed.id).await {
                                let mut idx = 0usize;
                                for d in &canonical.discs {
                                    for t in &d.tracks {
                                        expected.push(ExpectedTrackInput {
                                            index: idx,
                                            title: t.title.clone(),
                                            artist: Some(artist.into()),
                                            disc_number: Some(t.disc_number),
                                            track_number: Some(t.track_number),
                                            duration_ms: (t.duration_ms > 0).then_some(t.duration_ms),
                                        });
                                        idx += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                expected.push(ExpectedTrackInput {
                    index: 0,
                    title: title.into(),
                    artist: Some(artist.into()),
                    disc_number: None,
                    track_number: None,
                    duration_ms: None,
                });
            }

            let raw_search = search::search(artist, title, |_| {}).await;
            let hits_count = raw_search.as_ref().map(|r| r.hits.len()).unwrap_or(0);
            let healthy_hits: Vec<_> = raw_search
                .as_ref()
                .map(|r| r.hits.iter().filter(|h| h.rank >= 2 && hit_is_healthy(h)).take(4).collect())
                .unwrap_or_default();

            let res = resolve_album_torrents(artist, title, &expected, |_| {}).await;
            match res {
                Ok((resolutions, unavail)) => {
                    let best = resolutions.first();
                    let label = best.map(|b| b.display_label.as_str()).unwrap_or("NONE");
                    let matched = best.map(|b| b.matched_count).unwrap_or(0);
                    let total = expected.len();
                    let warn = best.and_then(|b| b.layout_warning.as_deref()).unwrap_or("");
                    if hits_count == 0 {
                        zero_hits += 1;
                    } else if resolutions.is_empty() {
                        inspect_failed_all += 1;
                    } else if best.is_some_and(|b| b.layout_warning.as_deref().is_some_and(|w| w.contains("CUE"))) {
                        cue_only += 1;
                    } else if total > 0 && matched == total {
                        full_match += 1;
                    } else if matched > 0 {
                        partial_match += 1;
                    } else {
                        inspect_failed_all += 1;
                    }
                    let tier_summary: Vec<String> = resolutions
                        .iter()
                        .map(|r| format!("{:?}:{}/{} ({}s,{})", r.tier, r.matched_count, r.total_expected, r.seeders.unwrap_or(0), r.provider))
                        .collect();
                    println!(
                        "[{:02}/{}] {} - {} (exp={}, hits={}, healthy={}): tiers={:?} unavail={:?} best_warn={:?} [{}]",
                        i + 1, cases.len(), artist, title, total, hits_count, healthy_hits.len(), tier_summary, unavail, warn, label
                    );
                }
                Err(e) => {
                    println!("[{:02}/{}] {} - {} ERROR: {}", i + 1, cases.len(), artist, title, e);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
        println!(
            "SUMMARY: total={} full={} partial={} cue={} zero_hits={} inspect_or_match_fail={}",
            cases.len(), full_match, partial_match, cue_only, zero_hits, inspect_failed_all
        );
    }
}
