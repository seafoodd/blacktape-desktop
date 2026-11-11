import { invoke } from "@tauri-apps/api/core";
import type { Platform } from "@/shared/types.ts";

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

/** Mirrors the Rust `quality::ExpectedQuality`. */
export interface ExpectedQuality {
  label: string;
  lossless: boolean;
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
