import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import { platformLabel } from "@/shared/lib/remote/platforms.ts";
import type { AudioCandidate, AudioQualityTier } from "@/shared/types/canonical.ts";

const qualityRank: Record<AudioQualityTier, number> = {
  low: 0,
  standard_lossy: 1,
  high_lossy: 2,
  lossless: 3,
};

const qualityLabel: Record<AudioQualityTier, string> = {
  low: "Low",
  standard_lossy: "Standard",
  high_lossy: "High",
  lossless: "Lossless",
};

function sourceOf(candidate: AudioCandidate) {
  return candidate.source.kind === "http_stream"
    ? candidate.source.platform.toLowerCase()
    : candidate.source.kind;
}

function optionOf(candidate: AudioCandidate) {
  const source = sourceOf(candidate);
  const codec = candidate.codec.toUpperCase();
  const tier = candidate.quality_tier;
  return { key: JSON.stringify([source, codec, tier]), source, codec, tier };
}

export function albumSourceOptions(tracks: readonly AlbumTrackInfo[]) {
  const options = new Map<string, ReturnType<typeof optionOf> & { label: string; trackCount: number }>();
  for (const track of tracks) {
    const counted = new Set<string>();
    for (const candidate of track.candidates ?? []) {
      const option = optionOf(candidate);
      if (counted.has(option.key)) continue;
      counted.add(option.key);
      const existing = options.get(option.key);
      if (existing) {
        existing.trackCount++;
      } else {
        const label = option.source === "torrent" ? "Torrent"
          : option.source === "local_file" ? "Local file" : platformLabel(option.source);
        options.set(option.key, {
          ...option,
          label: [label, option.codec, qualityLabel[option.tier]].filter(Boolean).join(" · "),
          trackCount: 1,
        });
      }
    }
  }
  return [...options.values()].sort((a, b) =>
    qualityRank[b.tier] - qualityRank[a.tier] || a.label.localeCompare(b.label),
  );
}

export type AlbumSourceOption = ReturnType<typeof albumSourceOptions>[number];

function closestCandidate(candidates: readonly AudioCandidate[], option: AlbumSourceOption) {
  const exact = candidates.findIndex((candidate) => optionOf(candidate).key === option.key);
  if (exact >= 0) return exact;

  const requestedRank = qualityRank[option.tier];
  const compare = (a: AudioCandidate, b: AudioCandidate) => {
    const aRank = qualityRank[a.quality_tier];
    const bRank = qualityRank[b.quality_tier];
    return Math.abs(aRank - requestedRank) - Math.abs(bRank - requestedRank)
      || bRank - aRank
      || Number(sourceOf(b) === option.source) - Number(sourceOf(a) === option.source)
      || Number(b.codec.toUpperCase() === option.codec) - Number(a.codec.toUpperCase() === option.codec)
      || Number(b.is_strict_match) - Number(a.is_strict_match);
  };
  return candidates.reduce((best, candidate, index) =>
    best < 0 || compare(candidate, candidates[best]) < 0 ? index : best,
  -1);
}

export function applyAlbumSource(tracks: readonly AlbumTrackInfo[], option: AlbumSourceOption): AlbumTrackInfo[] {
  return tracks.map((track) => {
    const candidates = track.candidates ?? [];
    const index = closestCandidate(candidates, option);
    const candidate = candidates[index];
    if (!candidate) return track;
    const url = candidate.source.kind === "http_stream" ? candidate.source.url : track.url;
    if (index === track.selected_candidate_idx && url === track.url) return track;
    return { ...track, selected_candidate_idx: index, url };
  });
}
