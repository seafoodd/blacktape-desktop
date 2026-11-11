import { describe, it, expect, vi } from "vitest";
import { createLatestGuard } from "./searchGuard.ts";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";

const { searchPlatformsMock } = vi.hoisted(() => ({
  searchPlatformsMock: vi.fn(),
}));

vi.mock("@/shared/lib/search/search.ts", () => ({
  searchPlatforms: searchPlatformsMock,
}));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("createLatestGuard", () => {
  it("reports no token as latest before any begin()", () => {
    const guard = createLatestGuard();
    expect(guard.isLatest(0)).toBe(false);
    expect(guard.isLatest(1)).toBe(false);
  });

  it("returns the token from begin() as the latest", () => {
    const guard = createLatestGuard();
    const token = guard.begin();
    expect(guard.isLatest(token)).toBe(true);
  });

  it("issues strictly increasing tokens", () => {
    const guard = createLatestGuard();
    const a = guard.begin();
    const b = guard.begin();
    const c = guard.begin();
    expect(b).toBeGreaterThan(a);
    expect(c).toBeGreaterThan(b);
  });

  it("invalidates earlier tokens once a newer request begins", () => {
    const guard = createLatestGuard();
    const first = guard.begin();
    const second = guard.begin();
    expect(guard.isLatest(first)).toBe(false);
    expect(guard.isLatest(second)).toBe(true);
  });

  it("keeps only the most recent token latest after many requests", () => {
    const guard = createLatestGuard();
    const tokens = [guard.begin(), guard.begin(), guard.begin(), guard.begin()];
    const last = tokens[tokens.length - 1];
    for (const t of tokens.slice(0, -1)) {
      expect(guard.isLatest(t)).toBe(false);
    }
    expect(guard.isLatest(last)).toBe(true);
  });

  it("returns false for a token that was never issued", () => {
    const guard = createLatestGuard();
    guard.begin();
    expect(guard.isLatest(999)).toBe(false);
    expect(guard.isLatest(-1)).toBe(false);
  });

  it("keeps independent guards independent", () => {
    const left = createLatestGuard();
    const right = createLatestGuard();
    const leftToken = left.begin();
    const rightToken = right.begin();
    expect(left.isLatest(leftToken)).toBe(true);
    expect(right.isLatest(rightToken)).toBe(true);
    expect(left.isLatest(rightToken)).toBe(rightToken === leftToken);
  });

  it("drops a slow response for an older query when a newer one has started", () => {
    // Mirrors executeSearch: each call takes a token, and only the latest token may write results.
    const guard = createLatestGuard();
    let visible: string[] = [];

    const slowOld = guard.begin(); // "a", response arrives last
    const fastNew = guard.begin(); // "ab", response arrives first

    // Newer response lands first and is applied.
    if (guard.isLatest(fastNew)) visible = ["ab result"];

    // Older response lands later and must not overwrite.
    if (guard.isLatest(slowOld)) visible = ["a result"];

    expect(visible).toEqual(["ab result"]);
  });

  it("lets the latest response through even when it arrives after an older one", () => {
    const guard = createLatestGuard();
    let visible: string[] = [];

    const first = guard.begin();
    const second = guard.begin();

    if (guard.isLatest(first)) visible = ["first"];
    if (guard.isLatest(second)) visible = ["second"];

    expect(visible).toEqual(["second"]);
  });
});

describe("executeSearch stale-response guard", () => {
  it("does not let an older query's late response overwrite newer results, but still caches it", async () => {
    const pending: Record<string, (value: SearchSuggestion[]) => void> = {};
    searchPlatformsMock.mockImplementation(
      (query: string) =>
        new Promise<SearchSuggestion[]>((resolve) => {
          pending[query] = resolve;
        }),
    );

    const { useLibraryStore } = await import("@/shared/store/libraryStore.ts");
    const { executeSearch } = useLibraryStore.getState();

    const olderRun = executeSearch("guardtest older");
    const newerRun = executeSearch("guardtest newer");

    const newerItems = [{ name: "newer" }] as unknown as SearchSuggestion[];
    const olderItems = [{ name: "older" }] as unknown as SearchSuggestion[];

    pending["guardtest newer"](newerItems);
    await newerRun;
    expect(useLibraryStore.getState().searchResults).toEqual(newerItems);

    pending["guardtest older"](olderItems);
    await olderRun;

    expect(useLibraryStore.getState().searchResults).toEqual(newerItems);
    expect(useLibraryStore.getState().searchCache["guardtest older"]).toEqual(
      olderItems,
    );
  });
});
