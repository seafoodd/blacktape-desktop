/** What each search source is doing while a search runs. Mirrors `src-tauri/src/search/progress.rs`. */

export type SearchSource = "youtube" | "bandcamp" | "soundcloud" | "musicbrainz";

export interface SourceProgressEvent {
  source: SearchSource;
  state: "waiting" | "running" | "done" | "failed";
  /** How long the source took to answer or fail. */
  elapsedMs: number | null;
  /** Results it returned. */
  count: number | null;
}

export interface SourceProgress extends Omit<SourceProgressEvent, "state"> {
  /** "unknown": the search ended without saying how this source did. */
  state: SourceProgressEvent["state"] | "unknown";
  /** `Date.now()` when the running time shown for it began. */
  startedAt: number | null;
}

export interface SearchRun {
  query: string;
  startedAt: number;
  finishedAt: number | null;
  /** Answered from the results kept from an earlier search: no source was asked. */
  cached: boolean;
  sources: SourceProgress[];
}

const SOURCE_ORDER: SearchSource[] = ["youtube", "bandcamp", "soundcloud", "musicbrainz"];

export const SOURCE_NAMES: Record<SearchSource, string> = {
  youtube: "YouTube",
  bandcamp: "Bandcamp",
  soundcloud: "SoundCloud",
  musicbrainz: "MusicBrainz",
};

/** Adds or replaces the entry of the event's source; sources keep one fixed order. */
export function applyProgress(
  sources: SourceProgress[],
  event: SourceProgressEvent,
  now: number,
): SourceProgress[] {
  const next: SourceProgress = { ...event, startedAt: event.state === "running" ? now : null };
  return [...sources.filter((s) => s.source !== event.source), next].sort(
    (a, b) => SOURCE_ORDER.indexOf(a.source) - SOURCE_ORDER.indexOf(b.source),
  );
}

/** For a search that has ended: nothing in it is still running. */
export function closeProgress(sources: SourceProgress[]): SourceProgress[] {
  return sources.map((s) =>
    s.state === "waiting" || s.state === "running" ? { ...s, state: "unknown", startedAt: null } : s,
  );
}

export function formatElapsed(ms: number): string {
  const whole = Math.max(0, Math.round(ms));
  return whole < 1000 ? `${whole} ms` : `${(whole / 1000).toFixed(1)} s`;
}
