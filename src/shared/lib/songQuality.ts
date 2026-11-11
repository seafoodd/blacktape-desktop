import type { QualityTier } from "@/shared/types.ts";

const TIER_LABELS: Record<QualityTier, string> = {
  low: "Low",
  standard: "Standard",
  high: "High",
  lossless: "Lossless",
};

export interface SongQuality {
  tier: QualityTier;
  /** "MP3 · Low", "OPUS · High", "FLAC" (a lossless file needs no tier word). */
  label: string;
  /** Tooltip: "Quality: Low (MP3)". */
  title: string;
}

/** File format of a song from its path ("C:\a\b.flac" -> "FLAC"); null without an extension. Pure. */
export function fileFormat(path: string): string | null {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toUpperCase();
}

/** Quality badge of a library song: its file format and the tier the library gave it. Pure. */
export function songQuality(song: { path: string; quality_tier: QualityTier }): SongQuality {
  const tier: QualityTier = song.quality_tier in TIER_LABELS ? song.quality_tier : "low";
  const tierLabel = TIER_LABELS[tier];
  const container = fileFormat(song.path);
  // a lossless .m4a / .mp4 holds ALAC: name the codec, not the container
  const format = tier === "lossless" && (container === "M4A" || container === "MP4") ? "ALAC" : container;
  const label = !format ? tierLabel : tier === "lossless" ? format : `${format} · ${tierLabel}`;
  return { tier, label, title: format ? `Quality: ${tierLabel} (${format})` : `Quality: ${tierLabel}` };
}
