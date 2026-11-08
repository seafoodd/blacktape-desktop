const KNOWN_PROVIDERS: Record<string, string> = {
  "musixmatch.com": "Musixmatch",
  "lrclib.net": "LRCLIB",
  "lyrics.ovh": "lyrics.ovh",
  "genius.com": "Genius",
};

/** Human-readable provider name for a lyrics source URL (e.g. "Musixmatch"). */
export function providerName(source: string): string {
  try {
    const host = new URL(source).hostname.toLowerCase().replace(/^www\./, "");
    for (const [domain, name] of Object.entries(KNOWN_PROVIDERS)) {
      if (host === domain || host.endsWith(`.${domain}`)) return name;
    }
    return host || source;
  } catch {
    // Not a URL (e.g. "Manual" from the edit dialog): show as is.
    return source;
  }
}
