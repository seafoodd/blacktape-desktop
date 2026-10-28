export interface LyricLine {
  text: string;
  timeMs?: number;
}

export interface ParsedLyrics {
  isTimed: boolean;
  lines: LyricLine[];
}

/**
 * Parses raw lyric text which might be in LRC format (with timestamps)
 * or plain text (without timestamps).
 */
export function parseLyrics(rawLyrics: string): ParsedLyrics {
  if (!rawLyrics || !rawLyrics.trim()) {
    return { isTimed: false, lines: [] };
  }

  const rawLines = rawLyrics.split(/\r?\n/);
  const timeRegex = /\[(\d{1,2}):(\d{2})(?:[.:](\d{1,3}))?\]/g;
  const metaRegex = /^\[[a-zA-Z]+:[^\]]*\]$/;

  const timedLines: LyricLine[] = [];
  let foundAnyTimestamps = false;

  for (const rawLine of rawLines) {
    const trimmed = rawLine.trim();
    if (!trimmed) continue;

    // Ignore metadata tags like [ti:Title], [ar:Artist], etc.
    if (metaRegex.test(trimmed)) {
      continue;
    }

    const matches = Array.from(trimmed.matchAll(timeRegex));
    if (matches.length > 0) {
      foundAnyTimestamps = true;
      const text = trimmed.replace(timeRegex, "").trim();

      for (const match of matches) {
        const minutes = parseInt(match[1], 10);
        const seconds = parseInt(match[2], 10);
        const fractionRaw = match[3] || "0";
        // Pad or trim fraction to 3 digits (milliseconds)
        const ms = parseInt(fractionRaw.padEnd(3, "0").slice(0, 3), 10);
        const timeMs = minutes * 60 * 1000 + seconds * 1000 + ms;

        timedLines.push({
          text,
          timeMs,
        });
      }
    }
  }

  if (foundAnyTimestamps && timedLines.length > 0) {
    // Sort chronologically
    timedLines.sort((a, b) => (a.timeMs ?? 0) - (b.timeMs ?? 0));
    return {
      isTimed: true,
      lines: timedLines,
    };
  }

  // Otherwise, treat as plain untimed lyrics
  const plainLines: LyricLine[] = rawLines
    .map((line) => ({ text: line.trim() }))
    .filter((line, idx, arr) => {
      // Keep non-empty lines, or at most one consecutive empty line
      if (line.text.length > 0) return true;
      return idx > 0 && arr[idx - 1].text.length > 0;
    });

  return {
    isTimed: false,
    lines: plainLines,
  };
}
