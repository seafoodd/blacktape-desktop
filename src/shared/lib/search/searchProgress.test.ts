import { describe, it, expect } from "vitest";
import {
  applyProgress,
  closeProgress,
  formatElapsed,
  type SourceProgress,
} from "@/shared/lib/search/searchProgress.ts";

const entry = (over: Partial<SourceProgress> & Pick<SourceProgress, "source">): SourceProgress => ({
  state: "waiting",
  elapsedMs: null,
  count: null,
  startedAt: null,
  ...over,
});

describe("applyProgress", () => {
  it("adds the source of the event", () => {
    const next = applyProgress(
      [],
      { source: "youtube", state: "waiting", elapsedMs: null, count: null },
      100,
    );
    expect(next).toEqual([entry({ source: "youtube", state: "waiting" })]);
  });

  it("replaces the entry of the same source and keeps the others", () => {
    const start = [
      entry({ source: "youtube", state: "running", startedAt: 5 }),
      entry({ source: "bandcamp", state: "waiting" }),
    ];
    const next = applyProgress(
      start,
      { source: "youtube", state: "done", elapsedMs: 300, count: 4 },
      900,
    );
    expect(next).toEqual([
      { source: "youtube", state: "done", elapsedMs: 300, count: 4, startedAt: null },
      entry({ source: "bandcamp", state: "waiting" }),
    ]);
  });

  it("keeps youtube, bandcamp, soundcloud, musicbrainz order whatever order events come in", () => {
    let sources: SourceProgress[] = [];
    const events = [
      { source: "musicbrainz" as const, state: "done" as const, elapsedMs: 1, count: 0 },
      { source: "soundcloud" as const, state: "failed" as const, elapsedMs: 2, count: null },
      { source: "youtube" as const, state: "running" as const, elapsedMs: null, count: null },
      { source: "bandcamp" as const, state: "waiting" as const, elapsedMs: null, count: null },
    ];
    for (const event of events) sources = applyProgress(sources, event, 0);
    expect(sources.map((s) => s.source)).toEqual([
      "youtube",
      "bandcamp",
      "soundcloud",
      "musicbrainz",
    ]);

    // Replacing an entry does not move it.
    sources = applyProgress(
      sources,
      { source: "youtube", state: "done", elapsedMs: 9, count: 2 },
      0,
    );
    expect(sources.map((s) => s.source)).toEqual([
      "youtube",
      "bandcamp",
      "soundcloud",
      "musicbrainz",
    ]);
  });

  it("sets startedAt to now only for a running source", () => {
    const next = applyProgress(
      [],
      { source: "youtube", state: "running", elapsedMs: null, count: null },
      1234,
    );
    expect(next[0].startedAt).toBe(1234);

    const done = applyProgress(
      next,
      { source: "youtube", state: "done", elapsedMs: 10, count: 1 },
      5000,
    );
    expect(done[0].startedAt).toBeNull();

    const waiting = applyProgress(
      [],
      { source: "bandcamp", state: "waiting", elapsedMs: null, count: null },
      5000,
    );
    expect(waiting[0].startedAt).toBeNull();
  });

  it("does not mutate its input", () => {
    const start: SourceProgress[] = [entry({ source: "youtube", state: "waiting" })];
    const snapshot = structuredClone(start);
    const next = applyProgress(
      start,
      { source: "youtube", state: "running", elapsedMs: null, count: null },
      10,
    );
    expect(start).toEqual(snapshot);
    expect(next).not.toBe(start);
    expect(next[0]).not.toBe(start[0]);
  });
});

describe("closeProgress", () => {
  it("turns waiting and running sources into unknown with no start time", () => {
    const closed = closeProgress([
      entry({ source: "youtube", state: "running", startedAt: 42 }),
      entry({ source: "bandcamp", state: "waiting" }),
    ]);
    expect(closed).toEqual([
      { source: "youtube", state: "unknown", elapsedMs: null, count: null, startedAt: null },
      { source: "bandcamp", state: "unknown", elapsedMs: null, count: null, startedAt: null },
    ]);
  });

  it("leaves done and failed sources untouched", () => {
    const start: SourceProgress[] = [
      { source: "youtube", state: "done", elapsedMs: 120, count: 7, startedAt: null },
      { source: "soundcloud", state: "failed", elapsedMs: 80, count: null, startedAt: null },
    ];
    expect(closeProgress(start)).toEqual(start);
  });

  it("does not mutate its input", () => {
    const start: SourceProgress[] = [entry({ source: "youtube", state: "running", startedAt: 1 })];
    const snapshot = structuredClone(start);
    closeProgress(start);
    expect(start).toEqual(snapshot);
  });
});

describe("formatElapsed", () => {
  it.each([
    [0, "0 ms"],
    [850.4, "850 ms"],
    [999.6, "1.0 s"],
    [1234, "1.2 s"],
    [-5, "0 ms"],
  ])("formats %d as %s", (ms, text) => {
    expect(formatElapsed(ms)).toBe(text);
  });
});
