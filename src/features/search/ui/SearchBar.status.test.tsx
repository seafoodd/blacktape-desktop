import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SearchRun, SearchSource, SourceProgress } from "@/shared/lib/search/searchProgress.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { SearchBar } from "./SearchBar.tsx";

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => actual.useState(initial === "" ? "the predator" : initial),
  };
});

vi.mock("@/features/search/model/searchProgressStyle.ts", () => ({
  useSearchProgressStyle: () => "pulse",
}));

vi.mock("@/shared/store/libraryStore.ts", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/shared/store/libraryStore.ts")>();
  const useLibraryStoreForRender = Object.assign(
    () => actual.useLibraryStore.getState(),
    actual.useLibraryStore,
  );
  return { ...actual, useLibraryStore: useLibraryStoreForRender };
});

const searchBarCss = readFileSync(new URL("./search-bar.module.css", import.meta.url), "utf8");

const source = (name: SearchSource, state: SourceProgress["state"]): SourceProgress => ({
  source: name,
  state,
  elapsedMs: null,
  count: null,
  startedAt: state === "running" ? 1_000 : null,
});

const run = (finishedAt: number | null): SearchRun => ({
  query: "the predator",
  startedAt: 1_000,
  finishedAt,
  cached: false,
  sources: [source("bandcamp", "done"), source("youtube", finishedAt === null ? "running" : "done")],
});

describe("search field progress visibility", () => {
  beforeEach(() => {
    useLibraryStore.setState({ searchResults: [], searchWarning: null, partialResultsFor: null });
  });

  it("keeps a finished run rendered while marking the indicators inactive", () => {
    useLibraryStore.setState({ searchRun: run(2_000) });
    const html = renderToStaticMarkup(<SearchBar />);

    expect(html).toContain('data-searching="false"');
    expect(html).toContain('aria-label="Search source progress"');
    expect(html).toContain('aria-label="Search sources"');
  });

  it("marks unfinished runs active even when the local searching flag is false", () => {
    useLibraryStore.setState({ searchRun: run(null) });
    const html = renderToStaticMarkup(<SearchBar />);

    expect(html).toContain('data-searching="true"');
    expect(html).toContain('aria-label="Search source progress"');
    expect(html).toContain('aria-label="Search sources"');
  });

  it("hides the whole indicator outside focus and active searches without capturing pointer input", () => {
    expect(searchBarCss).toMatch(/\.pulseIndicators\s*\{[^}]*pointer-events:\s*none;[^}]*opacity:\s*0;[^}]*visibility:\s*hidden;/s);
    expect(searchBarCss).toMatch(/\.searchFieldWrapper:focus-within \.pulseIndicators,\s*\.pulseIndicators\[data-searching="true"\]\s*\{[^}]*opacity:\s*1;[^}]*visibility:\s*visible;/s);
  });

  it("reserves the progress area and fades visibility without shifting the search field", () => {
    expect(searchBarCss).toMatch(/\.inputWithPulse\s*\{[^}]*padding-right:\s*150px;/s);
    expect(searchBarCss).toMatch(/\.pulseIndicators\s*\{[^}]*position:\s*absolute;[^}]*inset:\s*0;[^}]*transition:\s*opacity 180ms ease, visibility 0s linear 180ms;/s);
    expect(searchBarCss).toContain("@media (prefers-reduced-motion: reduce)");
    expect(searchBarCss).toMatch(/@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.pulseIndicators\s*\{\s*transition:\s*none;/s);
  });
});
