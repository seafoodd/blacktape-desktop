import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  useNavigationStore,
  isSameNavEntry,
  applyNavEntry,
  initNavigationListener,
} from "./navigationStore.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";

const setActiveViewMock = vi.fn();
const setSelectedTabMock = vi.fn().mockResolvedValue(undefined);
const selectPlaylistMock = vi.fn().mockResolvedValue(undefined);
const searchOpenMock = vi.fn();
const albumOpenMock = vi.fn().mockResolvedValue(undefined);

let libState: any = {
  activeView: "ARTIST_ALBUMS",
  selectedTab: "",
  setSelectedTab: setSelectedTabMock,
  setActiveView: setActiveViewMock,
};
let playlistState: any = {
  activePlaylistId: null,
  selectPlaylist: selectPlaylistMock,
};
let searchState: any = {
  query: "",
  results: [],
  activeTab: "all",
  isLoading: false,
  error: null,
  hasSearched: false,
  open: searchOpenMock,
};
let albumState: any = {
  item: null,
  preview: null,
  isLoading: false,
  error: null,
  selectedTracks: new Set<number>(),
  lastSelectedIndex: null,
  editing: false,
  editDraft: null,
  editions: [],
  editionsLoading: false,
  open: albumOpenMock,
};

const libSubscribers = new Set<(state: any, prev: any) => void>();
const playlistSubscribers = new Set<(state: any, prev: any) => void>();
const searchSubscribers = new Set<(state: any, prev: any) => void>();
const albumSubscribers = new Set<(state: any, prev: any) => void>();

vi.mock("@/shared/store/libraryStore.ts", () => ({
  useLibraryStore: {
    getState: vi.fn(() => libState),
    setState: vi.fn((patch: any) => {
      const prev = { ...libState };
      libState = { ...libState, ...(typeof patch === "function" ? patch(libState) : patch) };
      libSubscribers.forEach((fn) => fn(libState, prev));
    }),
    subscribe: vi.fn((fn: any) => {
      libSubscribers.add(fn);
      return () => libSubscribers.delete(fn);
    }),
  },
}));

vi.mock("@/shared/store/playlistStore.ts", () => ({
  usePlaylistStore: {
    getState: vi.fn(() => playlistState),
    setState: vi.fn((patch: any) => {
      const prev = { ...playlistState };
      playlistState = { ...playlistState, ...(typeof patch === "function" ? patch(playlistState) : patch) };
      playlistSubscribers.forEach((fn) => fn(playlistState, prev));
    }),
    subscribe: vi.fn((fn: any) => {
      playlistSubscribers.add(fn);
      return () => playlistSubscribers.delete(fn);
    }),
  },
}));

vi.mock("@/features/search/model/searchPageStore.ts", () => ({
  useSearchPageStore: {
    getState: vi.fn(() => searchState),
    setState: vi.fn((patch: any) => {
      const prev = { ...searchState };
      searchState = { ...searchState, ...(typeof patch === "function" ? patch(searchState) : patch) };
      searchSubscribers.forEach((fn) => fn(searchState, prev));
    }),
    subscribe: vi.fn((fn: any) => {
      searchSubscribers.add(fn);
      return () => searchSubscribers.delete(fn);
    }),
  },
}));

vi.mock("@/features/album-preview/model/albumPreviewStore.ts", () => ({
  useAlbumPreviewStore: {
    getState: vi.fn(() => albumState),
    setState: vi.fn((patch: any) => {
      const prev = { ...albumState };
      albumState = { ...albumState, ...(typeof patch === "function" ? patch(albumState) : patch) };
      albumSubscribers.forEach((fn) => fn(albumState, prev));
    }),
    subscribe: vi.fn((fn: any) => {
      albumSubscribers.add(fn);
      return () => albumSubscribers.delete(fn);
    }),
  },
}));

describe("navigationStore", () => {
  beforeEach(() => {
    libState = {
      activeView: "ARTIST_ALBUMS",
      selectedTab: "",
      setSelectedTab: setSelectedTabMock,
      setActiveView: setActiveViewMock,
    };
    playlistState = {
      activePlaylistId: null,
      selectPlaylist: selectPlaylistMock,
    };
    searchState = {
      query: "",
      sections: [],
      profile: "balanced",
      filters: {},
      sort: "relevance",
      open: searchOpenMock,
    };
    albumState = {
      status: "idle",
      item: null,
      preview: null,
      originalMetadata: null,
      editions: [],
      resolvingCandidates: false,
      isEditingMetadata: false,
      open: albumOpenMock,
    };
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

  describe("snapshot preservation and listener tracking", () => {
    it("restores exact ALBUM_PREVIEW and SEARCH_RESULTS snapshots on goBack/goForward without re-fetching", async () => {
      const searchItemA = {
        id: "alb-a",
        item_type: "album",
        title: "Album A",
        artist: "Artist A",
        item_url_path: "https://example.test/album-a",
        platform: "bandcamp",
      } as any;
      const previewA = {
        platform: "bandcamp",
        url: "https://example.test/album-a",
        title: "Album A",
        artist: "Artist A",
        cover_url: "cover-a.jpg",
        year: 2020,
        genres: [],
        tracks: [],
      } as any;
      const editionsA = [
        { id: "rel-a", title: "Album A", edition_label: "CD", year: 2020, track_count: 10, disc_count: 1, format: "CD", is_default: true },
      ];
      const searchSectionsA = [{ title: "Albums", items: [searchItemA] }] as any;

      const unsub = initNavigationListener();
      try {
        // 1. User opens search results
        searchState = {
          ...searchState,
          query: "aphex",
          sections: searchSectionsA,
          profile: "deep",
        };
        libState = { ...libState, activeView: "SEARCH_RESULTS" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "ARTIST_ALBUMS", selectedTab: "" }));

        // 2. User opens Album A from search results
        albumState = {
          ...albumState,
          status: "ready",
          item: searchItemA,
          preview: previewA,
          editions: editionsA,
        };
        libState = { ...libState, activeView: "ALBUM_PREVIEW" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "SEARCH_RESULTS", selectedTab: "" }));

        // 3. User switches directly to Album B while still in ALBUM_PREVIEW
        const searchItemB = { ...searchItemA, id: "alb-b", title: "Album B", item_url_path: "https://example.test/album-b" };
        const previewB = { ...previewA, title: "Album B", url: "https://example.test/album-b" };
        const prevAlbumState = { ...albumState };
        albumState = {
          ...albumState,
          status: "ready",
          item: searchItemB,
          preview: previewB,
          editions: [],
        };
        albumSubscribers.forEach((fn) => fn(albumState, prevAlbumState));

        const nav = useNavigationStore.getState();
        expect(nav.history.map((h) => h.view)).toEqual([
          "ARTIST_ALBUMS",
          "SEARCH_RESULTS",
          "ALBUM_PREVIEW",
          "ALBUM_PREVIEW",
        ]);
        expect(nav.currentIndex).toBe(3);

        // Go back from Album B -> should restore Album A's exact snapshot without calling albumOpenMock
        await useNavigationStore.getState().goBack();
        expect(albumState.preview).toEqual(previewA);
        expect(albumState.editions).toEqual(editionsA);
        expect(albumOpenMock).not.toHaveBeenCalled();

        // Go back from Album A -> should restore SEARCH_RESULTS exact snapshot without re-searching
        await useNavigationStore.getState().goBack();
        expect(searchState.query).toBe("aphex");
        expect(searchState.profile).toBe("deep");
        expect(searchState.sections).toEqual(searchSectionsA);
        expect(searchOpenMock).not.toHaveBeenCalled();

        // Go forward -> restores Album A again
        await useNavigationStore.getState().goForward();
        expect(albumState.preview).toEqual(previewA);
      } finally {
        unsub();
      }
    });

    it("records SEARCH_RESULTS query changes and PLAYLIST switches within the same view", () => {
      const unsub = initNavigationListener();
      try {
        searchState = { ...searchState, query: "boards" };
        libState = { ...libState, activeView: "SEARCH_RESULTS" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "ARTIST_ALBUMS", selectedTab: "" }));

        const prevSearch = { ...searchState };
        searchState = { ...searchState, query: "autechre" };
        searchSubscribers.forEach((fn) => fn(searchState, prevSearch));

        playlistState = { ...playlistState, activePlaylistId: 10 };
        libState = { ...libState, activeView: "PLAYLIST" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "SEARCH_RESULTS", selectedTab: "" }));

        const prevPlaylist = { ...playlistState };
        playlistState = { ...playlistState, activePlaylistId: 20 };
        playlistSubscribers.forEach((fn) => fn(playlistState, prevPlaylist));

        const history = useNavigationStore.getState().history;
        expect(history.map((h) => [h.view, h.searchQuery ?? h.playlistId])).toEqual([
          ["ARTIST_ALBUMS", undefined],
          ["SEARCH_RESULTS", "boards"],
          ["SEARCH_RESULTS", "autechre"],
          ["PLAYLIST", 10],
          ["PLAYLIST", 20],
        ]);
      } finally {
        unsub();
      }
    });

    it("handles overlapping goBack calls cleanly via navGuard without leaving isApplyingHistory stuck", async () => {
      let resolveSlowTab!: () => void;
      setSelectedTabMock.mockImplementationOnce(
        () =>
          new Promise<void>((r) => {
            resolveSlowTab = r;
          }),
      );

      useNavigationStore.getState().record({ view: "ARTIST_ALBUMS", artistName: "Aphex Twin" });
      useNavigationStore.getState().record({ view: "TRASH" });
      expect(useNavigationStore.getState().currentIndex).toBe(2);

      const first = useNavigationStore.getState().goBack();
      const second = useNavigationStore.getState().goBack();
      resolveSlowTab();
      await Promise.all([first, second]);

      expect(useNavigationStore.getState().currentIndex).toBe(0);
      expect(useNavigationStore.getState().isApplyingHistory).toBe(false);
    });

    it("updates background AlbumPreview candidate resolution in history so Back->Forward never spins forever", async () => {
      const searchItem = {
        id: "alb-doors",
        item_type: "album",
        title: "The Doors",
        artist: "The Doors",
        item_url_path: "canonical:rg:doors",
        platform: "youtube",
      } as any;
      const unresolvedPreview = {
        platform: "youtube",
        url: "canonical:rg:doors",
        title: "The Doors",
        artist: "The Doors",
        cover_url: "doors.jpg",
        year: 1967,
        genres: [],
        tracks: [{ index: 0, title: "Break On Through", artists: [], track_number: 1, url: "", available: true, note: null, candidates: [] }],
      } as any;

      const unsub = initNavigationListener();
      try {
        // 1. User opens AlbumPreview while candidates are still resolving
        albumState = {
          ...albumState,
          status: "ready",
          item: searchItem,
          preview: unresolvedPreview,
          resolvingCandidates: true,
        };
        libState = { ...libState, activeView: "ALBUM_PREVIEW" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "ARTIST_ALBUMS", selectedTab: "" }));

        // 2. User goes Back BEFORE resolution finishes
        await useNavigationStore.getState().goBack();
        expect(useNavigationStore.getState().currentIndex).toBe(0);

        // 3. Resolution finishes in the background while user is on Home
        const resolvedCand = { source: { kind: "http_stream", platform: "youtube", url: "https://yt/bot" }, quality_tier: "high_lossy", codec: "Opus", duration_ms: 149_000, match_confidence: 1, is_strict_match: true, display_label: "YT" };
        useNavigationStore.getState().syncAlbumCandidates("canonical:rg:doors", (prev) => ({
          preview: {
            ...prev,
            tracks: prev.tracks.map((t, i) => (i === 0 ? { ...t, url: "https://yt/bot", candidates: [resolvedCand as any] } : t)),
          },
          resolvingCandidates: true,
        }));
        useNavigationStore.getState().syncAlbumCandidatesDone("canonical:rg:doors");

        // 4. User goes Forward back to The Doors: must NOT restore resolvingCandidates=true!
        await useNavigationStore.getState().goForward();
        expect(albumState.resolvingCandidates).toBe(false);
        expect(albumState.preview.tracks[0].candidates).toHaveLength(1);
      } finally {
        unsub();
      }
    });

    it("keeps SearchResults snapshot synced when sections finish after user navigated to an album", async () => {
      const searchItem = {
        id: "alb-1",
        item_type: "album",
        title: "Drukqs",
        artist: "Aphex Twin",
        item_url_path: "canonical:rg:drukqs",
        platform: "youtube",
      } as any;
      const loadingSections = {
        Track: { status: "loading", items: [] },
        Album: { status: "ready", items: [searchItem] },
        Artist: { status: "ready", items: [] },
      };
      const completedSections = {
        Track: { status: "ready", items: [{ id: "tr-1", title: "Avril 14th" }] },
        Album: { status: "ready", items: [searchItem] },
        Artist: { status: "ready", items: [] },
      };

      const unsub = initNavigationListener();
      try {
        searchState = { ...searchState, query: "drukqs", sections: loadingSections };
        libState = { ...libState, activeView: "SEARCH_RESULTS" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "ARTIST_ALBUMS", selectedTab: "" }));

        // Navigate into album while Track search is still loading
        albumState = {
          ...albumState,
          status: "ready",
          item: searchItem,
          preview: { title: "Drukqs", url: "canonical:rg:drukqs", artist: "Aphex Twin", platform: "youtube", tracks: [], genres: [], cover_url: null, year: 2001 },
        };
        libState = { ...libState, activeView: "ALBUM_PREVIEW" };
        libSubscribers.forEach((fn) => fn(libState, { activeView: "SEARCH_RESULTS", selectedTab: "" }));

        // Track search completes while user is on ALBUM_PREVIEW
        const prevSearch = { ...searchState };
        searchState = { ...searchState, sections: completedSections };
        searchSubscribers.forEach((fn) => fn(searchState, prevSearch));

        // User goes Back to Search Results: Track section must already be "ready", not stuck "loading"
        await useNavigationStore.getState().goBack();
        expect(searchState.sections.Track.status).toBe("ready");
      } finally {
        unsub();
      }
    });

    it("restores saved startup nav entry via restoreSavedNav", async () => {
      await useNavigationStore.getState().restoreSavedNav({ view: "FAVORITES" });
      expect(useNavigationStore.getState().history).toEqual([{ view: "FAVORITES" }]);
      expect(useNavigationStore.getState().currentIndex).toBe(0);
      expect(useNavigationStore.getState().isApplyingHistory).toBe(false);
      expect(setActiveViewMock).toHaveBeenCalledWith("FAVORITES");
    });
  });
});
