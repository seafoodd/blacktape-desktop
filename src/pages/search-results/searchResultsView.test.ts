import { describe, it, expect, vi } from "vitest";
import {
  ALL_TAB_PREVIEW_LIMIT,
  durationFilterEnabled,
  filtersForKind,
  isKindShown,
  isProfileVisible,
  yearFilterEnabled,
} from "./searchResultsView.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import type { SearchPageFilters } from "@/features/search/model/searchPageFilters.ts";

// The view module imports the ItemType enum from the library store, which pulls in the Tauri and audio layers.
vi.mock("@/shared/lib/search/search.ts", () => ({ searchPlatforms: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({
  getArtistAlbums: vi.fn(),
  getArtists: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const filters: SearchPageFilters = {
  tab: "All",
  platform: "All",
  year: { min: 2000, max: 2010 },
  durationSecs: { min: 60, max: 300 },
  text: "daft",
};

describe("filtersForKind", () => {
  it("drops the year for tracks (tracks carry no year) and keeps duration, platform and text", () => {
    const f = filtersForKind(filters, ItemType.Track);
    expect(f.year).toEqual({});
    expect(f.durationSecs).toEqual({ min: 60, max: 300 });
    expect(f.text).toBe("daft");
  });

  it("keeps the year for albums and drops the duration", () => {
    const f = filtersForKind(filters, ItemType.Album);
    expect(f.year).toEqual({ min: 2000, max: 2010 });
    expect(f.durationSecs).toEqual({});
  });

  it("drops year and duration for artists but keeps platform and text", () => {
    const f = filtersForKind({ ...filters, platform: "youtube" }, ItemType.Artist);
    expect(f.year).toEqual({});
    expect(f.durationSecs).toEqual({});
    expect(f.platform).toBe("youtube");
    expect(f.text).toBe("daft");
  });

  it("does not mutate the input", () => {
    const snapshot = JSON.stringify(filters);
    filtersForKind(filters, ItemType.Artist);
    filtersForKind(filters, ItemType.Album);
    expect(JSON.stringify(filters)).toBe(snapshot);
  });
});

describe("ALL_TAB_PREVIEW_LIMIT", () => {
  it("shows 8 tracks and 12 albums or artists on the All tab", () => {
    expect(ALL_TAB_PREVIEW_LIMIT[ItemType.Track]).toBe(8);
    expect(ALL_TAB_PREVIEW_LIMIT[ItemType.Album]).toBe(12);
    expect(ALL_TAB_PREVIEW_LIMIT[ItemType.Artist]).toBe(12);
  });
});

describe("isKindShown", () => {
  it("shows every section on the All tab", () => {
    expect(isKindShown("All", ItemType.Track)).toBe(true);
    expect(isKindShown("All", ItemType.Album)).toBe(true);
    expect(isKindShown("All", ItemType.Artist)).toBe(true);
  });

  it("shows only the selected kind on a single tab", () => {
    expect(isKindShown(ItemType.Track, ItemType.Track)).toBe(true);
    expect(isKindShown(ItemType.Track, ItemType.Album)).toBe(false);
    expect(isKindShown(ItemType.Artist, ItemType.Artist)).toBe(true);
    expect(isKindShown(ItemType.Artist, ItemType.Track)).toBe(false);
  });
});

describe("filter enablement per tab", () => {
  it("enables the year range on All and Albums only", () => {
    expect(yearFilterEnabled("All")).toBe(true);
    expect(yearFilterEnabled(ItemType.Album)).toBe(true);
    expect(yearFilterEnabled(ItemType.Track)).toBe(false);
    expect(yearFilterEnabled(ItemType.Artist)).toBe(false);
  });

  it("enables the duration range on All and Tracks only", () => {
    expect(durationFilterEnabled("All")).toBe(true);
    expect(durationFilterEnabled(ItemType.Track)).toBe(true);
    expect(durationFilterEnabled(ItemType.Album)).toBe(false);
    expect(durationFilterEnabled(ItemType.Artist)).toBe(false);
  });
});

describe("isProfileVisible", () => {
  it("is visible on All and Artists while loading or ready", () => {
    expect(isProfileVisible("All", "All", "ready", "youtube")).toBe(true);
    expect(isProfileVisible(ItemType.Artist, "All", "loading", "youtube")).toBe(true);
  });

  it("is hidden on the Tracks and Albums tabs", () => {
    expect(isProfileVisible(ItemType.Track, "All", "ready", "youtube")).toBe(false);
    expect(isProfileVisible(ItemType.Album, "All", "ready", "youtube")).toBe(false);
  });

  it("is hidden when the profile failed or was never requested", () => {
    expect(isProfileVisible("All", "All", "error", "youtube")).toBe(false);
    expect(isProfileVisible("All", "All", "idle", "youtube")).toBe(false);
  });

  it("is hidden when the platform filter excludes the artist's platform", () => {
    expect(isProfileVisible("All", "youtube", "ready", "bandcamp")).toBe(false);
    expect(isProfileVisible("All", "bandcamp", "ready", "bandcamp")).toBe(true);
  });

  it("with a platform filter but no known artist platform, is hidden", () => {
    expect(isProfileVisible("All", "youtube", "ready", null)).toBe(false);
    expect(isProfileVisible("All", "All", "ready", null)).toBe(true);
  });
});
