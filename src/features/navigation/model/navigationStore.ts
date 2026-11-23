import { create } from "zustand";
import {
  useLibraryStore,
  ActiveView,
  SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { useSearchPageStore } from "@/features/search/model/searchPageStore.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";

export interface NavEntry {
  view: ActiveView;
  artistName?: string;
  playlistId?: number;
  searchQuery?: string;
  previewItem?: SearchSuggestion;
}

const MAX_HISTORY = 50;

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

async function applyNavEntry(entry: NavEntry): Promise<void> {
  switch (entry.view) {
    case "ARTIST_ALBUMS": {
      if (entry.artistName) {
        await useLibraryStore.getState().setSelectedTab(entry.artistName);
      } else {
        useLibraryStore.setState({
          selectedTab: "",
          albums: [],
          activeView: "ARTIST_ALBUMS",
        });
      }
      break;
    }
    case "SEARCH_RESULTS": {
      if (entry.searchQuery) {
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
      if (entry.previewItem) {
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
  }
}

interface NavigationStoreState {
  history: NavEntry[];
  currentIndex: number;
  isApplyingHistory: boolean;
  canGoBack: () => boolean;
  canGoForward: () => boolean;
  record: (entry: NavEntry) => void;
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
  },

  goBack: async () => {
    const { history, currentIndex, canGoBack } = get();
    if (!canGoBack()) return;

    const targetIndex = currentIndex - 1;
    const targetEntry = history[targetIndex];
    if (!targetEntry) return;

    set({ isApplyingHistory: true, currentIndex: targetIndex });
    try {
      await applyNavEntry(targetEntry);
    } finally {
      set({ isApplyingHistory: false });
    }
  },

  goForward: async () => {
    const { history, currentIndex, canGoForward } = get();
    if (!canGoForward()) return;

    const targetIndex = currentIndex + 1;
    const targetEntry = history[targetIndex];
    if (!targetEntry) return;

    set({ isApplyingHistory: true, currentIndex: targetIndex });
    try {
      await applyNavEntry(targetEntry);
    } finally {
      set({ isApplyingHistory: false });
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

      if (state.activeView === "SEARCH_RESULTS") {
        searchQuery = useSearchPageStore.getState().query;
      } else if (state.activeView === "PLAYLIST") {
        playlistId = usePlaylistStore.getState().activePlaylistId ?? undefined;
      } else if (state.activeView === "ALBUM_PREVIEW") {
        previewItem = useAlbumPreviewStore.getState().item ?? undefined;
      }

      navStore.record({
        view: state.activeView,
        artistName:
          state.activeView === "ARTIST_ALBUMS" ? state.selectedTab : undefined,
        searchQuery,
        playlistId,
        previewItem,
      });
    }
  });

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.altKey && e.key === "ArrowLeft") {
      e.preventDefault();
      void useNavigationStore.getState().goBack();
    } else if (e.altKey && e.key === "ArrowRight") {
      e.preventDefault();
      void useNavigationStore.getState().goForward();
    }
  };

  if (typeof window !== "undefined") {
    window.addEventListener("keydown", handleKeyDown);
  }

  return () => {
    unsubLibrary();
    if (typeof window !== "undefined") {
      window.removeEventListener("keydown", handleKeyDown);
    }
  };
}
