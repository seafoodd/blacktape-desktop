/** A word with its own timing (Enhanced LRC / richsync). */
export interface LyricWord {
  text: string;
  startMs: number;
  /** Absent when the source gives no end time (use the next word / line end). */
  endMs?: number;
}

export interface LyricLine {
  text: string;
  timeMs?: number;
  /** Present only when the source has word-level timing. */
  words?: LyricWord[];
}

export interface ParsedLyrics {
  isTimed: boolean;
  lines: LyricLine[];
}

const WORD_TAG_REGEX = /<(\d{1,2}):(\d{2})(?:[.:](\d{1,3}))?>/g;

function toMs(min: string, sec: string, frac?: string): number {
  const ms = parseInt((frac || "0").padEnd(3, "0").slice(0, 3), 10);
  return parseInt(min, 10) * 60_000 + parseInt(sec, 10) * 1000 + ms;
}

/**
 * Parses the part of a line after its `[mm:ss.xx]` stamp as Enhanced LRC:
 * `<mm:ss.xx>word <mm:ss.xx>word <mm:ss.xx>` (trailing tag = end of line).
 * Plain text without tags yields `words: undefined` and unchanged text.
 */
export function parseWordTags(content: string): {
  text: string;
  words?: LyricWord[];
} {
  const tags = Array.from(content.matchAll(WORD_TAG_REGEX));
  if (tags.length === 0) return { text: content.trim() };

  const words: LyricWord[] = [];
  for (let i = 0; i < tags.length; i++) {
    const tag = tags[i];
    const segStart = (tag.index ?? 0) + tag[0].length;
    const segEnd = tags[i + 1]?.index ?? content.length;
    const text = content.slice(segStart, segEnd).trim();
    if (!text) continue;
    const startMs = toMs(tag[1], tag[2], tag[3]);
    const next = tags[i + 1];
    words.push({
      text,
      startMs,
      endMs: next ? toMs(next[1], next[2], next[3]) : undefined,
    });
  }

  const text = (words.length > 0
    ? words.map((w) => w.text).join(" ")
    : content.replace(WORD_TAG_REGEX, "")
  ).trim();
  return words.length > 0 ? { text, words } : { text };
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
      const { text, words } = parseWordTags(trimmed.replace(timeRegex, ""));

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
          // Word times are absolute, so they only make sense for a single stamp.
          ...(words && matches.length === 1 ? { words } : {}),
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
