import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { SearchRun, SearchSource, SourceProgress } from "@/shared/lib/search/searchProgress.ts";
import { SearchProgress } from "./SearchProgress.tsx";

const source = (name: SearchSource, state: SourceProgress["state"]): SourceProgress => ({
  source: name,
  state,
  elapsedMs: state === "failed" || state === "done" ? 25 : null,
  count: state === "done" ? 3 : null,
  startedAt: state === "running" ? 1_000 : null,
});

const run = (sources: SourceProgress[]): SearchRun => ({
  query: "Radiohead",
  startedAt: 1_000,
  finishedAt: 2_000,
  cached: false,
  sources,
});

describe("SearchProgress retry chips", () => {
  it("offers accessible retry buttons for failed and unknown sources, but not healthy sources", () => {
    const html = renderToStaticMarkup(
      <SearchProgress
        run={run([
          source("bandcamp", "failed"),
          source("soundcloud", "unknown"),
          source("youtube", "done"),
        ])}
        onRetry={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="Retry Bandcamp"');
    expect(html).toContain('aria-label="Retry SoundCloud"');
    expect(html).not.toContain('aria-label="Retry YouTube"');
  });

  it("disables retry buttons while another source is still running", () => {
    const html = renderToStaticMarkup(
      <SearchProgress
        run={run([source("soundcloud", "failed"), source("youtube", "running")])}
        onRetry={vi.fn()}
      />,
    );

    expect(html).toMatch(/<button(?=[^>]*aria-label="Retry SoundCloud")(?=[^>]*disabled="")[^>]*>/);
  });

  it("hides retry buttons when no retry action is provided", () => {
    const html = renderToStaticMarkup(<SearchProgress run={run([source("soundcloud", "failed")])} />);
    expect(html).not.toContain('aria-label="Retry SoundCloud"');
    expect(html).not.toContain(">Retry</button>");
  });
});
