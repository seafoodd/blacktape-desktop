import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  useNavigationStore,
  isSameNavEntry,
  applyNavEntry,
} from "./navigationStore.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";

const setActiveViewMock = vi.fn();
const setSelectedTabMock = vi.fn().mockResolvedValue(undefined);

vi.mock("@/shared/store/libraryStore.ts", () => ({
  useLibraryStore: {
    getState: vi.fn(() => ({
      setSelectedTab: setSelectedTabMock,
      setActiveView: setActiveViewMock,
    })),
    setState: vi.fn(),
  },
}));

vi.mock("@/shared/store/playlistStore.ts", () => ({
  usePlaylistStore: {
    getState: vi.fn(() => ({
      selectPlaylist: vi.fn().mockResolvedValue(undefined),
    })),
  },
}));

vi.mock("@/features/search/model/searchPageStore.ts", () => ({
  useSearchPageStore: {
    getState: vi.fn(() => ({
      open: vi.fn(),
    })),
  },
}));

vi.mock("@/features/album-preview/model/albumPreviewStore.ts", () => ({
  useAlbumPreviewStore: {
    getState: vi.fn(() => ({
      open: vi.fn().mockResolvedValue(undefined),
    })),
  },
}));

describe("navigationStore", () => {
  beforeEach(() => {
    useNavigationStore.getState().reset({
      view: "ARTIST_ALBUMS",
      artistName: "",
    });
    vi.clearAllMocks();
  });

  describe("isSameNavEntry", () => {
    it("returns false if either entry is undefined", () => {
      expect(isSameNavEntry(undefined, { view: "TRASH" })).toBe(false);
      expect(isSameNavEntry({ view: "TRASH" }, undefined)).toBe(false);
    });

    it("returns false for different views", () => {
      expect(
        isSameNavEntry({ view: "TRASH" }, { view: "ARTIST_ALBUMS" }),
      ).toBe(false);
    });

    it("compares ARTIST_ALBUMS entries by artistName", () => {
      expect(
        isSameNavEntry(
          { view: "ARTIST_ALBUMS", artistName: "Radiohead" },
          { view: "ARTIST_ALBUMS", artistName: "Radiohead" },
        ),
      ).toBe(true);
      expect(
        isSameNavEntry(
          { view: "ARTIST_ALBUMS", artistName: "Radiohead" },
          { view: "ARTIST_ALBUMS", artistName: "Daft Punk" },
        ),
      ).toBe(false);
      expect(
        isSameNavEntry(
          { view: "ARTIST_ALBUMS" },
          { view: "ARTIST_ALBUMS", artistName: "" },
        ),
      ).toBe(true);
    });

    it("compares PLAYLIST entries by playlistId", () => {
      expect(
        isSameNavEntry(
          { view: "PLAYLIST", playlistId: 1 },
          { view: "PLAYLIST", playlistId: 1 },
        ),
      ).toBe(true);
      expect(
        isSameNavEntry(
          { view: "PLAYLIST", playlistId: 1 },
          { view: "PLAYLIST", playlistId: 2 },
        ),
      ).toBe(false);
    });

    it("compares SEARCH_RESULTS entries by searchQuery", () => {
      expect(
        isSameNavEntry(
          { view: "SEARCH_RESULTS", searchQuery: "rock" },
          { view: "SEARCH_RESULTS", searchQuery: "rock" },
        ),
      ).toBe(true);
      expect(
        isSameNavEntry(
          { view: "SEARCH_RESULTS", searchQuery: "rock" },
          { view: "SEARCH_RESULTS", searchQuery: "ambient" },
        ),
      ).toBe(false);
    });

    it("compares ALBUM_PREVIEW entries by item_url_path", () => {
      const item1 = {
        item_url_path: "/album/1",
        title: "Album 1",
        platform: "youtube",
        item_type: "album",
      } as any;
      const item2 = {
        item_url_path: "/album/2",
        title: "Album 2",
        platform: "youtube",
        item_type: "album",
      } as any;

      expect(
        isSameNavEntry(
          { view: "ALBUM_PREVIEW", previewItem: item1 },
          { view: "ALBUM_PREVIEW", previewItem: item1 },
        ),
      ).toBe(true);
      expect(
        isSameNavEntry(
          { view: "ALBUM_PREVIEW", previewItem: item1 },
          { view: "ALBUM_PREVIEW", previewItem: item2 },
        ),
      ).toBe(false);
    });

    it("applies FAVORITES entry", async () => {
      await applyNavEntry({ view: "FAVORITES" });
      expect(setActiveViewMock).toHaveBeenCalledWith("FAVORITES");
    });

    it("compares FAVORITES entries", () => {
      expect(
        isSameNavEntry({ view: "FAVORITES" }, { view: "FAVORITES" }),
      ).toBe(true);
      expect(
        isSameNavEntry({ view: "FAVORITES" }, { view: "TRASH" }),
      ).toBe(false);
    });
  });

  describe("history management", () => {
    it("starts with one entry and canGoBack=false, canGoForward=false", () => {
      const store = useNavigationStore.getState();
      expect(store.history).toHaveLength(1);
      expect(store.currentIndex).toBe(0);
      expect(store.canGoBack()).toBe(false);
      expect(store.canGoForward()).toBe(false);
    });

    it("records a new entry and updates canGoBack", () => {
      const store = useNavigationStore.getState();
      store.record({ view: "SEARCH_RESULTS", searchQuery: "ambient" });

      const updated = useNavigationStore.getState();
      expect(updated.history).toHaveLength(2);
      expect(updated.currentIndex).toBe(1);
      expect(updated.canGoBack()).toBe(true);
      expect(updated.canGoForward()).toBe(false);
    });

    it("does not record duplicate adjacent entries", () => {
      const store = useNavigationStore.getState();
      store.record({ view: "SEARCH_RESULTS", searchQuery: "ambient" });
      store.record({ view: "SEARCH_RESULTS", searchQuery: "ambient" });

      const updated = useNavigationStore.getState();
      expect(updated.history).toHaveLength(2);
      expect(updated.currentIndex).toBe(1);
    });

    it("navigates back and forward correctly", async () => {
      const store = useNavigationStore.getState();
      store.record({ view: "SEARCH_RESULTS", searchQuery: "ambient" });
      store.record({ view: "TRASH" });

      expect(useNavigationStore.getState().currentIndex).toBe(2);
      expect(useNavigationStore.getState().canGoBack()).toBe(true);
      expect(useNavigationStore.getState().canGoForward()).toBe(false);

      await useNavigationStore.getState().goBack();
      expect(useNavigationStore.getState().currentIndex).toBe(1);
      expect(useNavigationStore.getState().canGoBack()).toBe(true);
      expect(useNavigationStore.getState().canGoForward()).toBe(true);

      await useNavigationStore.getState().goBack();
      expect(useNavigationStore.getState().currentIndex).toBe(0);
      expect(useNavigationStore.getState().canGoBack()).toBe(false);
      expect(useNavigationStore.getState().canGoForward()).toBe(true);

      await useNavigationStore.getState().goForward();
      expect(useNavigationStore.getState().currentIndex).toBe(1);
    });

    it("truncates forward history when recording after going back", async () => {
      const store = useNavigationStore.getState();
      store.record({ view: "SEARCH_RESULTS", searchQuery: "ambient" });
      store.record({ view: "TRASH" });

      await useNavigationStore.getState().goBack();
      expect(useNavigationStore.getState().currentIndex).toBe(1);

      // Now record a new entry while in the middle of history
      useNavigationStore
        .getState()
        .record({ view: "PLAYLIST", playlistId: 42 });

      const updated = useNavigationStore.getState();
      expect(updated.history).toHaveLength(3);
      expect(updated.history[2].view).toBe("PLAYLIST");
      expect(updated.currentIndex).toBe(2);
      expect(updated.canGoForward()).toBe(false);
    });

    it("handles goHome by navigating to Home view", async () => {
      const store = useNavigationStore.getState();
      store.record({ view: "TRASH" });

      expect(useNavigationStore.getState().currentIndex).toBe(1);

      await useNavigationStore.getState().goHome();

      const updated = useNavigationStore.getState();
      expect(updated.history[updated.currentIndex].view).toBe("ARTIST_ALBUMS");
      expect(updated.history[updated.currentIndex].artistName).toBe("");
      expect(useLibraryStore.setState).toHaveBeenCalledWith({
        selectedTab: "",
        albums: [],
        activeView: "ARTIST_ALBUMS",
      });
    });

    it("goHome is a no-op if already at home", async () => {
      expect(useNavigationStore.getState().currentIndex).toBe(0);
      await useNavigationStore.getState().goHome();
      expect(useNavigationStore.getState().history).toHaveLength(1);
    });
  });
});
