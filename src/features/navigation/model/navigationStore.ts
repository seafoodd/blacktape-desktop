import { create } from "zustand";
import {
  useLibraryStore,
  ActiveView,
  SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import {
  useSearchPageStore,
  type ProfileState,
  type SectionState,
} from "@/features/search/model/searchPageStore.ts";
import type { SearchKind } from "@/shared/lib/search/searchFull.ts";
import type { SearchPageFilters, SearchPageSort } from "@/features/search/model/searchPageFilters.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import type { AlbumPreview, MetadataSnapshot } from "@/shared/lib/remote/albumPreview.ts";
import type { ReleaseEditionOption } from "@/shared/types/canonical.ts";
import { saveNavEntry } from "@/features/player/model/playerPersistence.ts";
import { createLatestGuard } from "@/shared/lib/search/searchGuard.ts";
import { EMPTY_SELECTION } from "@/shared/lib/ui/selection.ts";

interface AlbumPreviewSnapshot {
  item: SearchSuggestion | null;
  preview: AlbumPreview;
  originalMetadata: MetadataSnapshot | null;
  editions: ReleaseEditionOption[];
  resolvingCandidates: boolean;
  isEditingMetadata: boolean;
}

interface SearchPageSnapshot {
  query: string;
  sections: Record<SearchKind, SectionState>;
  profile: ProfileState;
  filters: SearchPageFilters;
  sort: SearchPageSort;
}

export interface NavEntry {
  view: ActiveView;
  artistName?: string;
  playlistId?: number;
  searchQuery?: string;
  previewItem?: SearchSuggestion;
  previewSnapshot?: AlbumPreviewSnapshot;
  searchSnapshot?: SearchPageSnapshot;
}

const MAX_HISTORY = 50;
const navGuard = createLatestGuard();

function stripSnapshots(entry: NavEntry): NavEntry {
  const { previewSnapshot: _p, searchSnapshot: _s, ...serializable } = entry;
  return serializable;
}

export function isSameNavEntry(
  a: NavEntry | undefined,
  b: NavEntry | undefined,
): boolean {
  if (!a || !b) return false;
  if (a.view !== b.view) return false;
  if (a.view === "ARTIST_ALBUMS") {
    return (a.artistName ?? "") === (b.artistName ?? "");
  }
  if (a.view === "PLAYLIST") {
    return a.playlistId === b.playlistId;
  }
  if (a.view === "SEARCH_RESULTS") {
    return (a.searchQuery ?? "") === (b.searchQuery ?? "");
  }
  if (a.view === "ALBUM_PREVIEW") {
    return a.previewItem?.item_url_path === b.previewItem?.item_url_path;
  }
  return true;
}

function getInitialNavEntry(): NavEntry {
  let initialTab = "";
  try {
    initialTab = sessionStorage.getItem("selectedTab") || "";
  } catch {}
  return {
    view: "ARTIST_ALBUMS",
    artistName: initialTab,
  };
}

export async function applyNavEntry(entry: NavEntry): Promise<void> {
  switch (entry.view) {
    case "ARTIST_ALBUMS": {
      if (entry.artistName) {
        await useLibraryStore.getState().setSelectedTab(entry.artistName);
      } else {
        useLibraryStore.getState().cancelTabLoad?.();
        useLibraryStore.setState({
          selectedTab: "",
          albums: [],
          activeView: "ARTIST_ALBUMS",
        });
      }
      break;
    }
    case "SEARCH_RESULTS": {
      if (
        entry.searchSnapshot &&
        entry.searchSnapshot.query === (entry.searchQuery ?? "")
      ) {
        useSearchPageStore.getState().invalidatePending?.();
        useSearchPageStore.setState({
          query: entry.searchSnapshot.query,
          sections: entry.searchSnapshot.sections,
          profile: entry.searchSnapshot.profile,
          filters: entry.searchSnapshot.filters,
          sort: entry.searchSnapshot.sort,
        });
        useLibraryStore.getState().setActiveView("SEARCH_RESULTS");
        useSearchPageStore.getState().resumeIncomplete?.();
      } else if (entry.searchQuery) {
        useSearchPageStore.getState().open(entry.searchQuery);
      } else {
        useLibraryStore.getState().setActiveView("SEARCH_RESULTS");
      }
      break;
    }
    case "PLAYLIST": {
      if (entry.playlistId !== undefined) {
        await usePlaylistStore.getState().selectPlaylist(entry.playlistId);
      }
      useLibraryStore.getState().setActiveView("PLAYLIST");
      break;
    }
    case "ALBUM_PREVIEW": {
      useAlbumPreviewStore.getState().invalidatePending?.();
      if (entry.previewSnapshot) {
        useAlbumPreviewStore.setState({
          item: entry.previewItem ?? entry.previewSnapshot.item,
          status: "ready",
          preview: entry.previewSnapshot.preview,
          originalMetadata: entry.previewSnapshot.originalMetadata,
          editions: entry.previewSnapshot.editions,
          resolvingCandidates: entry.previewSnapshot.resolvingCandidates,
          isEditingMetadata: entry.previewSnapshot.isEditingMetadata,
          error: null,
          loadingEditions: false,
          switchingRelease: false,
          selection: EMPTY_SELECTION,
        });
        useLibraryStore.getState().setActiveView("ALBUM_PREVIEW");
        void useAlbumPreviewStore.getState().resumeAfterRestore?.();
      } else if (entry.previewItem) {
        await useAlbumPreviewStore.getState().open(entry.previewItem);
      } else {
        useLibraryStore.getState().setActiveView("ALBUM_PREVIEW");
      }
      break;
    }
    case "TRASH": {
      useLibraryStore.getState().setActiveView("TRASH");
      break;
    }
    case "FAVORITES": {
      useLibraryStore.getState().setActiveView("FAVORITES");
      break;
    }
  }
}

interface NavigationStoreState {
  history: NavEntry[];
  currentIndex: number;
  isApplyingHistory: boolean;
  canGoBack: () => boolean;
  canGoForward: () => boolean;
  record: (entry: NavEntry) => void;
  updateCurrentSnapshot: (patch: Partial<Pick<NavEntry, "previewSnapshot" | "searchSnapshot">>) => void;
  syncAlbumSnapshotByItem: (itemUrlPath: string, snapshot: AlbumPreviewSnapshot) => void;
  syncAlbumCandidates: (
    albumUrl: string,
    updater: (preview: AlbumPreview) => { preview: AlbumPreview; resolvingCandidates: boolean } | null,
  ) => void;
  syncAlbumCandidatesDone: (albumUrl: string) => void;
  syncSearchSnapshotByQuery: (query: string, snapshot: SearchPageSnapshot) => void;
  restoreSavedNav: (entry: NavEntry) => Promise<void>;
  goBack: () => Promise<void>;
  goForward: () => Promise<void>;
  goHome: () => Promise<void>;
  reset: (initialEntry?: NavEntry) => void;
}

export const useNavigationStore = create<NavigationStoreState>((set, get) => ({
  history: [getInitialNavEntry()],
  currentIndex: 0,
  isApplyingHistory: false,

  canGoBack: () => get().currentIndex > 0,
  canGoForward: () => get().currentIndex < get().history.length - 1,

  record: (entry: NavEntry) => {
    const { history, currentIndex, isApplyingHistory } = get();
    if (isApplyingHistory) return;

    const current = history[currentIndex];
    if (isSameNavEntry(current, entry)) return;

    let newHistory = [...history.slice(0, currentIndex + 1), entry];
    let newIndex = newHistory.length - 1;

    if (newHistory.length > MAX_HISTORY) {
      newHistory = newHistory.slice(newHistory.length - MAX_HISTORY);
      newIndex = newHistory.length - 1;
    }

    set({
      history: newHistory,
      currentIndex: newIndex,
    });
    saveNavEntry(stripSnapshots(entry));
  },

  updateCurrentSnapshot: (patch) => {
    const { history, currentIndex, isApplyingHistory } = get();
    if (isApplyingHistory) return;
    const current = history[currentIndex];
    if (!current) return;
    const nextHistory = [...history];
    nextHistory[currentIndex] = { ...current, ...patch };
    set({ history: nextHistory });
  },

  syncAlbumSnapshotByItem: (itemUrlPath, snapshot) => {
    set((state) => {
      let changed = false;
      const nextHistory = state.history.map((entry) => {
        if (entry.view === "ALBUM_PREVIEW" && entry.previewItem?.item_url_path === itemUrlPath) {
          changed = true;
          return { ...entry, previewSnapshot: snapshot };
        }
        return entry;
      });
      return changed ? { history: nextHistory } : {};
    });
  },

  syncAlbumCandidates: (albumUrl, updater) => {
    set((state) => {
      let changed = false;
      const nextHistory = state.history.map((entry) => {
        if (
          entry.view === "ALBUM_PREVIEW" &&
          entry.previewSnapshot &&
          entry.previewSnapshot.preview.url === albumUrl
        ) {
          const updated = updater(entry.previewSnapshot.preview);
          if (!updated) return entry;
          changed = true;
          return {
            ...entry,
            previewSnapshot: {
              ...entry.previewSnapshot,
              preview: updated.preview,
              resolvingCandidates: updated.resolvingCandidates,
            },
          };
        }
        return entry;
      });
      return changed ? { history: nextHistory } : {};
    });
  },

  syncAlbumCandidatesDone: (albumUrl) => {
    set((state) => {
      let changed = false;
      const nextHistory = state.history.map((entry) => {
        if (
          entry.view === "ALBUM_PREVIEW" &&
          entry.previewSnapshot &&
          entry.previewSnapshot.preview.url === albumUrl &&
          entry.previewSnapshot.resolvingCandidates
        ) {
          changed = true;
          return {
            ...entry,
            previewSnapshot: {
              ...entry.previewSnapshot,
              resolvingCandidates: false,
            },
          };
        }
        return entry;
      });
      return changed ? { history: nextHistory } : {};
    });
  },

  syncSearchSnapshotByQuery: (query, snapshot) => {
    set((state) => {
      let changed = false;
      const nextHistory = state.history.map((entry) => {
        if (entry.view === "SEARCH_RESULTS" && entry.searchQuery === query) {
          changed = true;
          return { ...entry, searchSnapshot: snapshot };
        }
        return entry;
      });
      return changed ? { history: nextHistory } : {};
    });
  },

  restoreSavedNav: async (entry) => {
    const token = navGuard.begin();
    set({
      history: [entry],
      currentIndex: 0,
      isApplyingHistory: true,
    });
    try {
      await applyNavEntry(entry);
    } finally {
      if (navGuard.isLatest(token)) {
        set({ isApplyingHistory: false });
      }
    }
  },

  goBack: async () => {
    const { history, currentIndex, canGoBack } = get();
    if (!canGoBack()) return;

    const targetIndex = currentIndex - 1;
    const targetEntry = history[targetIndex];
    if (!targetEntry) return;

    const token = navGuard.begin();
    set({ isApplyingHistory: true, currentIndex: targetIndex });
    try {
      await applyNavEntry(targetEntry);
      if (navGuard.isLatest(token)) {
        saveNavEntry(stripSnapshots(targetEntry));
      }
    } finally {
      if (navGuard.isLatest(token)) {
        set({ isApplyingHistory: false });
      }
    }
  },

  goForward: async () => {
    const { history, currentIndex, canGoForward } = get();
    if (!canGoForward()) return;

    const targetIndex = currentIndex + 1;
    const targetEntry = history[targetIndex];
    if (!targetEntry) return;

    const token = navGuard.begin();
    set({ isApplyingHistory: true, currentIndex: targetIndex });
    try {
      await applyNavEntry(targetEntry);
      if (navGuard.isLatest(token)) {
        saveNavEntry(stripSnapshots(targetEntry));
      }
    } finally {
      if (navGuard.isLatest(token)) {
        set({ isApplyingHistory: false });
      }
    }
  },

  goHome: async () => {
    const { history, currentIndex } = get();
    const current = history[currentIndex];
    if (
      current?.view === "ARTIST_ALBUMS" &&
      (!current.artistName || current.artistName === "")
    ) {
      return;
    }

    const homeEntry: NavEntry = {
      view: "ARTIST_ALBUMS",
      artistName: "",
    };

    get().record(homeEntry);
    useLibraryStore.getState().cancelTabLoad?.();
    useLibraryStore.setState({
      selectedTab: "",
      albums: [],
      activeView: "ARTIST_ALBUMS",
    });
  },

  reset: (initialEntry?: NavEntry) => {
    set({
      history: [initialEntry ?? getInitialNavEntry()],
      currentIndex: 0,
      isApplyingHistory: false,
    });
  },
}));

function captureCurrentAlbumSnapshot(): AlbumPreviewSnapshot | undefined {
  const ap = useAlbumPreviewStore.getState();
  if (ap.status !== "ready" || !ap.preview) return undefined;
  return {
    item: ap.item,
    preview: ap.preview,
    originalMetadata: ap.originalMetadata,
    editions: ap.editions,
    resolvingCandidates: ap.resolvingCandidates,
    isEditingMetadata: ap.isEditingMetadata,
  };
}

function captureCurrentSearchSnapshot(): SearchPageSnapshot | undefined {
  const sp = useSearchPageStore.getState();
  if (!sp.query) return undefined;
  return {
    query: sp.query,
    sections: sp.sections,
    profile: sp.profile,
    filters: sp.filters,
    sort: sp.sort,
  };
}

export function initNavigationListener(): () => void {
  const unsubLibrary = useLibraryStore.subscribe((state, prevState) => {
    const navStore = useNavigationStore.getState();
    if (navStore.isApplyingHistory) return;

    if (
      state.activeView !== prevState.activeView ||
      (state.activeView === "ARTIST_ALBUMS" &&
        state.selectedTab !== prevState.selectedTab)
    ) {
      let searchQuery: string | undefined;
      let playlistId: number | undefined;
      let previewItem: SearchSuggestion | undefined;
      let previewSnapshot: AlbumPreviewSnapshot | undefined;
      let searchSnapshot: SearchPageSnapshot | undefined;

      if (state.activeView === "SEARCH_RESULTS") {
        searchQuery = useSearchPageStore.getState().query;
        searchSnapshot = captureCurrentSearchSnapshot();
      } else if (state.activeView === "PLAYLIST") {
        playlistId = usePlaylistStore.getState().activePlaylistId ?? undefined;
      } else if (state.activeView === "ALBUM_PREVIEW") {
        previewItem = useAlbumPreviewStore.getState().item ?? undefined;
        previewSnapshot = captureCurrentAlbumSnapshot();
      }

      navStore.record({
        view: state.activeView,
        artistName:
          state.activeView === "ARTIST_ALBUMS" ? state.selectedTab : undefined,
        searchQuery,
        playlistId,
        previewItem,
        previewSnapshot,
        searchSnapshot,
      });
    }
  });

  const unsubAlbumPreview = useAlbumPreviewStore.subscribe((state, prevState) => {
    const navStore = useNavigationStore.getState();
    if (navStore.isApplyingHistory) return;
    const activeView = useLibraryStore.getState().activeView;

    if (
      activeView === "ALBUM_PREVIEW" &&
      state.item &&
      state.item.item_url_path !== prevState.item?.item_url_path
    ) {
      navStore.record({
        view: "ALBUM_PREVIEW",
        previewItem: state.item,
        previewSnapshot: captureCurrentAlbumSnapshot(),
      });
      return;
    }

    if (state.item?.item_url_path && state.status === "ready" && state.preview) {
      const snap = captureCurrentAlbumSnapshot();
      if (!snap) return;
      const current = navStore.history[navStore.currentIndex];
      if (
        activeView === "ALBUM_PREVIEW" &&
        current?.view === "ALBUM_PREVIEW" &&
        current.previewItem?.item_url_path === state.item.item_url_path
      ) {
        navStore.updateCurrentSnapshot({ previewSnapshot: snap });
      } else {
        navStore.syncAlbumSnapshotByItem(state.item.item_url_path, snap);
      }
    }
  });

  const unsubSearchPage = useSearchPageStore.subscribe((state, prevState) => {
    const navStore = useNavigationStore.getState();
    if (navStore.isApplyingHistory) return;
    const activeView = useLibraryStore.getState().activeView;

    if (activeView === "SEARCH_RESULTS" && state.query && state.query !== prevState.query) {
      navStore.record({
        view: "SEARCH_RESULTS",
        searchQuery: state.query,
        searchSnapshot: captureCurrentSearchSnapshot(),
      });
      return;
    }

    if (state.query) {
      const snap = captureCurrentSearchSnapshot();
      if (!snap) return;
      const current = navStore.history[navStore.currentIndex];
      if (
        activeView === "SEARCH_RESULTS" &&
        current?.view === "SEARCH_RESULTS" &&
        current.searchQuery === state.query
      ) {
        navStore.updateCurrentSnapshot({ searchSnapshot: snap });
      } else {
        navStore.syncSearchSnapshotByQuery(state.query, snap);
      }
    }
  });

  const unsubPlaylist = usePlaylistStore.subscribe((state, prevState) => {
    const navStore = useNavigationStore.getState();
    if (navStore.isApplyingHistory) return;
    if (useLibraryStore.getState().activeView !== "PLAYLIST") return;

    if (
      state.activePlaylistId !== null &&
      state.activePlaylistId !== prevState.activePlaylistId
    ) {
      navStore.record({
        view: "PLAYLIST",
        playlistId: state.activePlaylistId,
      });
    }
  });

  return () => {
    unsubLibrary();
    unsubAlbumPreview();
    unsubSearchPage();
    unsubPlaylist();
  };
}
