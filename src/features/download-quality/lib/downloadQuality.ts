import { invoke } from "@tauri-apps/api/core";
import type { TorrentTierResolution } from "@/shared/lib/audio.ts";
import { audioTierRank, type AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { Platform } from "@/shared/types.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import { sameUrl } from "@/shared/lib/remote/urlKey.ts";

/** Mirrors the Rust `quality::AudioFormat` (snake_case on the wire). */
export interface AudioFormat {
  /** yt-dlp format id, passed back as `StartDownloadParams.format`. */
  id: string;
  codec: string;
  bitrate_kbps: number | null;
  sample_rate_hz: number | null;
  lossless: boolean;
  /** "FLAC (lossless)", "MP3 320 kbps", "Opus 131 kbps". */
  label: string;
}

interface CanonicalQualityOption {
  id: string;
  label: string;
  details: string;
  platform?: string;
  isBest?: boolean;
  lossless?: boolean;
  upgradableCount?: number;
  warning?: string | null;
  disabled?: boolean;
}

export interface CanonicalQualityPlan {
  best: CanonicalQualityOption;
  options: CanonicalQualityOption[];
  hasDiscoveredCandidates: boolean;
}

/** Mirrors the Rust `quality::ExpectedQuality`. */
export interface ExpectedQuality {
  label: string;
  lossless: boolean;
}

/** Label of the primary CTA button in QualityModal ("Download" when choosing a non-best preset). Pure. */
export function downloadButtonLabel(
  isMultiSource: boolean,
  picked: string | null,
  formats: AudioFormat[] | null,
): string {
  if (isMultiSource) {
    return picked === "best" ? "Download in the best quality" : "Download";
  }
  return formats !== null && formats.length === 0 ? "Download in the best quality" : "Download";
}

/** How long the format probe usually takes: shown so a waiting user does not think the app hung. */
export const PROBE_WAIT_TEXT = "This takes 5-10 seconds.";

/** Why a Bandcamp album often has only MP3 128 kbps, and what gives the better files. */
export const BANDCAMP_QUALITY_HINT =
  "Bandcamp streams every album as MP3 128 kbps. FLAC, ALAC, WAV and MP3 320 come with the download files, " +
  "which Bandcamp gives when the artist offers the album for free or for a price you name (you can enter 0), " +
  "or once you bought the album on Bandcamp.";

/** Expected quality without a probe, for the platforms that answer at once (Bandcamp asks the backend). Pure. */
export function expectedQualityOf(platform: Platform | string): ExpectedQuality | null {
  switch (platform.toLowerCase()) {
    case "youtube":
      return { label: "Opus ~160 kbps", lossless: false };
    case "soundcloud":
      return { label: "MP3 128 / AAC 160 kbps", lossless: false };
    default:
      return null;
  }
}

/** Expected quality of a platform album or track (Bandcamp reads the page, about 1 s; the rest at once). */
export async function getExpectedQuality(platform: Platform | string, url: string): Promise<ExpectedQuality> {
  return expectedQualityOf(platform) ?? invoke<ExpectedQuality>("get_expected_quality", { platform, url });
}

/** The formats yt-dlp finds for the track, or the first track of the album (about 3-6 s). */
export async function probeAudioFormats(
  platform: Platform | string,
  url: string,
  isAlbum: boolean,
  trackUrl?: string,
): Promise<AudioFormat[]> {
  return invoke<AudioFormat[]>("probe_audio_formats", {
    platform,
    url,
    isAlbum,
    trackUrl: trackUrl ?? null,
  });
}

/** Second line of a format row: "48 kHz" etc.; empty when yt-dlp gave nothing more than the label. Pure. */
export function formatDetails(format: AudioFormat): string {
  const parts: string[] = [];
  if (format.sample_rate_hz) parts.push(`${+(format.sample_rate_hz / 1000).toFixed(1)} kHz`);
  if (format.lossless) parts.push("lossless");
  return parts.join(" · ");
}

function candidatePlatformName(cand: NonNullable<AlbumTrackInfo["candidates"]>[number]): string {
  return cand.source.kind === "http_stream"
    ? cand.source.platform.toLowerCase()
    : cand.source.kind.toLowerCase();
}

function tierDisplayLabel(
  codec: string,
  tier: string,
  bitrateKbps: number | null | undefined,
): string {
  const upper = codec.toUpperCase();
  const displayCodec = upper === "OPUS" ? "Opus" : upper === "VORBIS" ? "Vorbis" : upper;
  if (tier === "lossless") {
    return /^(FLAC|WAV|ALAC|AIFF|APE|WAVPACK|WV|DSF|DFF|TAK|TTA|SHN|PCM)$/.test(upper)
      ? `${displayCodec} (Lossless)`
      : "Lossless";
  }
  if (bitrateKbps) return `${displayCodec} ${bitrateKbps} kbps`;
  if (tier === "high_lossy") return `${displayCodec} (High)`;
  if (tier === "standard_lossy") return `${displayCodec} (Standard)`;
  return displayCodec;
}

export function torrentQualityLabel(release: TorrentTierResolution): string {
  return release.tier === "lossless"
    ? tierDisplayLabel(release.codec, release.tier, release.bitrate_kbps)
    : release.display_label;
}

export function mergeScopedQualityTracks(
  selected: readonly AlbumTrackInfo[],
  resolved: readonly AlbumTrackInfo[],
): AlbumTrackInfo[] {
  return selected.map((track) => {
    const match = resolved.find((candidate) => candidate.index === track.index &&
      candidate.title.trim().toLowerCase() === track.title.trim().toLowerCase() &&
      candidate.disc_number === track.disc_number && candidate.track_number === track.track_number)
      ?? resolved.find((candidate) => candidate.title.trim().toLowerCase() === track.title.trim().toLowerCase() &&
        sameUrl(candidate.url, track.url));
    if (match?.candidates?.length) {
      const merged = [...match.candidates];
      for (const candidate of track.candidates ?? []) {
        if (!merged.some((other) => other.source.kind === "http_stream" && candidate.source.kind === "http_stream" &&
          sameUrl(other.source.url, candidate.source.url)) &&
          !merged.some((other) => other === candidate)) merged.push(candidate);
      }
      return { ...track, candidates: merged.sort(compareCandidates), selected_candidate_idx: 0 };
    }
    if (track.candidates?.length) return track;

    // The linked track's existing URL still provides its platform option while candidates load.
    let url: URL;
    try { url = new URL(track.url); } catch { return track; }
    if (url.protocol !== "https:") return track;
    const youtube = url.hostname === "youtu.be" ||
      (["www.youtube.com", "music.youtube.com", "youtube.com"].includes(url.hostname) && url.searchParams.has("v"));
    const bandcamp = url.hostname.endsWith(".bandcamp.com") && url.pathname.startsWith("/track/");
    const soundcloud = ["soundcloud.com", "www.soundcloud.com"].includes(url.hostname) &&
      url.pathname.split("/").filter(Boolean).length === 2;
    if (!youtube && !bandcamp && !soundcloud) return track;
    const platform = youtube ? "youtube" : bandcamp ? "bandcamp" : "soundcloud";
    return {
      ...track,
      candidates: [{
        source: { kind: "http_stream", platform, url: track.url },
        quality_tier: youtube ? "high_lossy" : "standard_lossy", codec: youtube ? "Opus" : "MP3",
        bitrate_kbps: youtube ? 160 : 128,
        duration_ms: track.duration_ms ?? 0, match_confidence: 1, is_strict_match: true,
        display_label: platform,
      }],
      selected_candidate_idx: 0,
    };
  });
}

/** Extracts unique numeric song IDs from `resolve:` URLs (`resolve:song:42`, `resolve:batch:...`, `resolve:album:...`). Pure. */
export function extractResolveSongIds(
  url: string,
  trackUrls?: readonly string[],
): number[] {
  const ids: number[] = [];
  const push = (id: number) => {
    if (Number.isInteger(id) && id > 0 && !ids.includes(id)) ids.push(id);
  };
  const scan = (raw: string) => {
    const songMatch = /^resolve:song:(\d+)$/.exec(raw.trim());
    if (songMatch) {
      push(Number(songMatch[1]));
      return;
    }
    const batchMatch = /^resolve:batch:([\d,]+)$/.exec(raw.trim());
    if (batchMatch) {
      for (const part of batchMatch[1].split(",")) {
        if (part) push(Number(part));
      }
    }
  };
  if (trackUrls) {
    for (const u of trackUrls) scan(u);
  }
  scan(url);
  return ids;
}

export function tierSuffixFor(tier: string): string {
  switch (tier) {
    case "lossless":
      return ":lossless";
    case "high_lossy":
      return ":high";
    case "standard_lossy":
      return ":standard";
    case "low":
      return ":low";
    default:
      return "";
  }
}

export function formatByteSize(bytes: number | null | undefined): string | null {
  if (!bytes || bytes <= 0) return null;
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `~${(mb / 1024).toFixed(1)} GB`;
  }
  return `~${Math.max(1, Math.round(mb))} MB`;
}

function compareCandidates(a: AudioCandidate, b: AudioCandidate): number {
  if (a.is_strict_match !== b.is_strict_match) {
    return a.is_strict_match ? -1 : 1;
  }
  const rankDiff = audioTierRank(b.quality_tier) - audioTierRank(a.quality_tier);
  if (rankDiff !== 0) return rankDiff;
  if (a.source.kind === "torrent" && b.source.kind === "torrent") return 0;

  // At equal lossless fidelity, prefer instant HTTP direct streams (e.g. Bandcamp free FLAC) over P2P torrents.
  if (a.source.kind !== b.source.kind && a.quality_tier === "lossless") {
    return a.source.kind === "http_stream" ? -1 : 1;
  }

  const bitrateDiff = (b.bitrate_kbps ?? 0) - (a.bitrate_kbps ?? 0);
  if (bitrateDiff !== 0) return bitrateDiff;

  return (b.match_confidence ?? 0) - (a.match_confidence ?? 0);
}

/** Merges matched candidates from resolved torrent releases onto tracks and ranks best sources first. Pure. */
export function mergeTorrentResolutionsIntoTracks(
  tracks: readonly AlbumTrackInfo[],
  resolutions: readonly TorrentTierResolution[],
): AlbumTrackInfo[] {
  if (resolutions.length === 0) return [...tracks];

  const byIndex = new Map<number, AudioCandidate[]>();
  for (const res of resolutions) {
    if (res.matched_count === 0) continue;
    for (const entry of res.tracks) {
      const list = byIndex.get(entry.expected_index) ?? [];
      list.push(entry.candidate);
      byIndex.set(entry.expected_index, list);
    }
  }
  if (byIndex.size === 0) return [...tracks];

  if (tracks.length === 0) {
    const out: AlbumTrackInfo[] = [];
    for (const [index, cands] of [...byIndex.entries()].sort((a, b) => a[0] - b[0])) {
      const sorted = [...cands].sort(compareCandidates);
      const top = sorted[0];
      out.push({
        index,
        title: top?.matched_title ?? `Track ${index + 1}`,
        artists: top?.matched_artist ? [top.matched_artist] : [],
        track_number: index + 1,
        url: top?.source.kind === "torrent" ? top.source.magnet_uri : "",
        available: sorted.length > 0,
        note: null,
        candidates: sorted,
        selected_candidate_idx: 0,
      });
    }
    return out;
  }

  return tracks.map((track) => {
    const incoming = byIndex.get(track.index);
    if (!incoming || incoming.length === 0) return track;

    const existing = track.candidates ?? [];
    const merged: AudioCandidate[] = [...existing];
    for (const cand of incoming) {
      if (cand.source.kind === "torrent") {
        const { info_hash, file_index } = cand.source;
        const alreadyPresent = merged.some(
          (m) =>
            m.source.kind === "torrent" &&
            m.source.info_hash === info_hash &&
            m.source.file_index === file_index,
        );
        if (!alreadyPresent) merged.push(cand);
      } else {
        merged.push(cand);
      }
    }
    if (merged.length === existing.length) return track;
    const selected = existing[track.selected_candidate_idx ?? 0];
    merged.sort(compareCandidates);
    const top = merged[0];
    const shouldPromoteTop =
      !selected ||
      (track.selected_candidate_idx ?? 0) === 0;
    const nextUrl =
      track.url || (top?.source.kind === "http_stream" ? top.source.url : top?.source.kind === "torrent" ? top.source.magnet_uri : "");
    return {
      ...track,
      available: track.available || merged.length > 0,
      url: nextUrl,
      candidates: merged,
      selected_candidate_idx: shouldPromoteTop ? 0 : merged.indexOf(selected),
    };
  });
}

/**
 * Builds the `Automatic Best Quality` card + discovered multi-source stream & torrent rows from `tracks`.
 * Honest: never claims "lossless" unless an actual Lossless candidate was resolved. Pure.
 */
export function buildCanonicalQualityPlan(
  tracks: readonly AlbumTrackInfo[],
  selectedUrls?: readonly string[],
  torrentResolutions?: readonly TorrentTierResolution[],
): CanonicalQualityPlan {
  const activeTracks = tracks.filter((t) => {
    if (!t.available || !t.url) return false;
    if (!selectedUrls || selectedUrls.length === 0) return true;
    return selectedUrls.includes(t.url);
  });

  const bestSummaryCounts = new Map<string, { label: string; count: number; lossless: boolean }>();
  const sourceGroups = new Map<
    string,
    {
      platform: string;
      tier: string;
      codec: string;
      bitrate: number | null;
      count: number;
      upgradableCount: number;
      tierRank: number;
    }
  >();

  let tracksWithCandidates = 0;
  let anyLossless = false;
  let anyHasLocalTier = false;
  let bestUpgradableCount = 0;

  const parseLocalTierRank = (note: string | null | undefined): number | null => {
    if (!note || !note.startsWith("local_tier:")) return null;
    return audioTierRank(note.slice("local_tier:".length));
  };

  for (const track of activeTracks) {
    const cands = track.candidates ?? [];
    if (cands.length === 0) continue;
    tracksWithCandidates += 1;

    const localRank = parseLocalTierRank(track.note);
    if (localRank !== null) anyHasLocalTier = true;

    const top = cands[0];
    if (top.quality_tier === "lossless") anyLossless = true;
    const topImproves = localRank === null || audioTierRank(top.quality_tier) > localRank;
    if (topImproves) {
      bestUpgradableCount += 1;
      const topPlat = candidatePlatformName(top);
      const topCodec = tierDisplayLabel(top.codec, top.quality_tier, top.bitrate_kbps);
      const topKey = `${topPlat}::${topCodec}`;
      const prevTop = bestSummaryCounts.get(topKey);
      if (prevTop) prevTop.count += 1;
      else {
        bestSummaryCounts.set(topKey, {
          label: `${topCodec} (${topPlat.toUpperCase()})`,
          count: 1,
          lossless: top.quality_tier === "lossless",
        });
      }
    }

    const seenInTrack = new Set<string>();
    for (const c of cands) {
      const plat = candidatePlatformName(c);
      const bucket = `${plat}::${c.quality_tier}`;
      if (seenInTrack.has(bucket)) continue;
      seenInTrack.add(bucket);
      const improves = localRank === null || audioTierRank(c.quality_tier) > localRank;
      const existing = sourceGroups.get(bucket);
      if (existing) {
        existing.count += 1;
        if (improves) existing.upgradableCount += 1;
      } else {
        sourceGroups.set(bucket, {
          platform: plat,
          tier: c.quality_tier,
          codec: c.codec,
          bitrate: c.bitrate_kbps ?? null,
          count: 1,
          upgradableCount: improves ? 1 : 0,
          tierRank: audioTierRank(c.quality_tier),
        });
      }
    }
  }

  if (tracksWithCandidates === 0) {
    return {
      best: {
        id: "best",
        label: "Automatic Best Quality",
        details: "Selects the best discovered quality per track across platforms",
        isBest: true,
        lossless: false,
      },
      options: [
        {
          id: "high",
          label: "High Quality (Opus 160k / AAC 256k)",
          details: "Prefer high-bitrate lossy streams",
          platform: "youtube",
        },
        {
          id: "standard",
          label: "Standard Quality (MP3 128k)",
          details: "Prefer compact 128 kbps streams",
          platform: "bandcamp",
        },
      ],
      hasDiscoveredCandidates: false,
    };
  }

  const totalCount = activeTracks.length;
  const bestParts = [...bestSummaryCounts.values()]
    .sort((a, b) => b.count - a.count)
    .slice(0, 3)
    .map((entry) =>
      totalCount > 1 ? `${entry.count}× ${entry.label}` : entry.label,
    );

  const sortedSources = [...sourceGroups.values()].sort(
    (a, b) => b.tierRank - a.tierRank || b.count - a.count || a.platform.localeCompare(b.platform),
  );

  const torrentsByTier = new Map<TorrentTierResolution["tier"], TorrentTierResolution[]>();
  for (const res of torrentResolutions ?? []) {
    if (res.matched_count <= 0) continue;
    const list = torrentsByTier.get(res.tier) ?? [];
    list.push(res);
    torrentsByTier.set(res.tier, list);
  }

  const options: CanonicalQualityOption[] = [];
  for (const src of sortedSources) {
    const tierSuffix = tierSuffixFor(src.tier);
    const tierTorrents = src.platform === "torrent"
      ? (torrentsByTier.get(src.tier as TorrentTierResolution["tier"]) ?? [])
      : [];

    if (src.platform === "torrent" && tierTorrents.length > 0) {
      tierTorrents.forEach((torrentRes, idx) => {
        const coverage =
          totalCount > 1 ? `${torrentRes.matched_count}/${totalCount} tracks` : "Verified candidate";
        const metaParts: string[] = [];
        if (anyHasLocalTier) {
          metaParts.push(
            src.upgradableCount === 0
              ? `${coverage} · Already at equal or higher quality`
              : src.upgradableCount < totalCount
                ? `Upgrades ${src.upgradableCount}/${totalCount} tracks (${totalCount - src.upgradableCount} skipped)`
                : totalCount > 1
                  ? `Upgrades ${src.upgradableCount}/${totalCount} tracks`
                  : "Upgrades existing file",
          );
        } else {
          metaParts.push(coverage);
        }
        const size = formatByteSize(torrentRes.total_matched_bytes);
        if (size) metaParts.push(size);
        if (torrentRes.seeders != null) {
          metaParts.push(`${torrentRes.seeders} seeder${torrentRes.seeders === 1 ? "" : "s"}`);
        }
        if (torrentRes.provider) {
          metaParts.push(torrentRes.provider);
        }
        const optId = idx === 0
          ? `source:${src.platform}${tierSuffix}`
          : `source:${src.platform}${tierSuffix || ":"}:${torrentRes.info_hash}`;
        options.push({
          id: optId,
          label: torrentQualityLabel(torrentRes),
          details: metaParts.join(" · "),
          platform: src.platform,
          lossless: src.tier === "lossless",
          upgradableCount: anyHasLocalTier ? src.upgradableCount : undefined,
          warning: torrentRes.layout_warning ?? null,
        });
      });
      continue;
    }

    const coverage =
      totalCount > 1 ? `${src.count}/${totalCount} tracks` : "Verified candidate";
    const metaParts: string[] = [];
    if (anyHasLocalTier) {
      metaParts.push(
        src.upgradableCount === 0
          ? `${coverage} · Already at equal or higher quality`
          : src.upgradableCount < totalCount
            ? `Upgrades ${src.upgradableCount}/${totalCount} tracks (${totalCount - src.upgradableCount} skipped)`
            : totalCount > 1
              ? `Upgrades ${src.upgradableCount}/${totalCount} tracks`
              : "Upgrades existing file",
      );
    } else {
      metaParts.push(coverage);
    }
    options.push({
      id: `source:${src.platform}${tierSuffix}`,
      label: tierDisplayLabel(src.codec, src.tier, src.bitrate),
      details: metaParts.join(" · "),
      platform: src.platform,
      lossless: src.tier === "lossless",
      upgradableCount: anyHasLocalTier ? src.upgradableCount : undefined,
      warning: null,
    });
  }

  // Include unsplit CUE-image notices at the end of options as informational disabled cards.
  for (const res of torrentResolutions ?? []) {
    if (res.matched_count === 0 && res.layout_warning) {
      options.push({
        id: `torrent-notice:${res.info_hash}`,
        label: res.display_label,
        details: `${res.provider}${res.seeders != null ? ` · ${res.seeders} seeders` : ""}`,
        platform: "torrent",
        lossless: res.tier === "lossless",
        warning: res.layout_warning,
        disabled: true,
      });
    }
  }

  const bestDetails = anyHasLocalTier
    ? bestUpgradableCount === 0
      ? "Already at equal or higher quality (no upgrade needed)"
      : bestUpgradableCount < totalCount
        ? `Upgrades ${bestUpgradableCount}/${totalCount}: ${bestParts.join(", ")}`
        : bestParts.length > 0
          ? `Upgrades to ${bestParts.join(", ")}`
          : "Picks highest quality per track"
    : bestParts.length > 0
      ? `Expected: ${bestParts.join(", ")}`
      : "Picks highest quality per track";

  return {
    best: {
      id: "best",
      label: "Automatic Best Quality",
      details: bestDetails,
      isBest: true,
      lossless: anyLossless,
      upgradableCount: anyHasLocalTier ? bestUpgradableCount : undefined,
    },
    options,
    hasDiscoveredCandidates: true,
  };
}
