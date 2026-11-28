import { Platform } from "@/shared/types.ts";

/** A known HTTP(S) hostname wins over the caller's platform hint. */
export function platformForUrl(raw: string, fallback: Platform): Platform {
  try {
    const url = new URL(raw.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") return fallback;
    const host = url.hostname.toLowerCase().replace(/\.+$/, "");
    const onDomain = (domain: string) => host === domain || host.endsWith(`.${domain}`);
    if (onDomain("youtube.com") || onDomain("youtu.be")) return "youtube";
    if (onDomain("bandcamp.com")) return "bandcamp";
    if (onDomain("soundcloud.com") || onDomain("snd.sc")) return "soundcloud";
  } catch {
    // Invalid URLs keep the caller's platform hint.
  }
  return fallback;
}

/** Values of the platform filter pills (search dropdown and search results page). */
export type PlatformFilter = "All" | "Youtube" | "Bandcamp" | "Soundcloud";

export const PLATFORM_FILTERS: PlatformFilter[] = [
  "All",
  "Youtube",
  "Bandcamp",
  "Soundcloud",
];

/** Platforms queried by the search bar. */
export const SEARCH_PLATFORMS: Platform[] = ["bandcamp", "youtube", "soundcloud"];

/** Platforms whose files were downloaded from a URL and can be downloaded again. */
export const REMOTE_PLATFORMS: Platform[] = ["youtube", "bandcamp", "soundcloud"];

const PLATFORM_LABELS: Record<Platform, string> = {
  youtube: "YouTube",
  bandcamp: "Bandcamp",
  soundcloud: "SoundCloud",
  local: "Local",
};

/** Human-readable platform name; unknown values are returned as-is. */
export const platformLabel = (platform: Platform | string): string => {
  const lower = platform.toLowerCase();
  if (lower === "torrent") return "Torrent";
  return PLATFORM_LABELS[lower as Platform] ?? platform;
};

/** Label of a filter pill ("All" -> "All Platforms"). */
export const platformFilterLabel = (filter: PlatformFilter): string =>
  filter === "All" ? "All Platforms" : platformLabel(filter);

/** Whether an item from `platform` passes the active platform filter. */
export const matchesPlatformFilter = (
  platform: Platform | string | undefined | null,
  filter: PlatformFilter,
): boolean =>
  filter === "All" || platform?.toLowerCase() === filter.toLowerCase();

/** A song can be re-downloaded when it came from a remote platform and kept its URL. */
export const canRedownload = (
  source: Platform | string | undefined | null,
  sourceUrl: string | undefined | null,
): boolean =>
  !!sourceUrl && REMOTE_PLATFORMS.includes(source?.toLowerCase() as Platform);
