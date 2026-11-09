import { create } from "zustand";
import { createLatestGuard } from "@/shared/lib/searchGuard.ts";
import {
  getAlbumPreview,
  selectableIndexes,
  selectedUrls,
  type AlbumPreview,
} from "@/shared/lib/albumPreview.ts";
import {
  EMPTY_SELECTION,
  nextSelection,
  selectGroup,
  type ClickModifiers,
  type SelectionState,
} from "@/shared/lib/selection.ts";
import { ItemType, useLibraryStore, type SearchSuggestion } from "@/shared/store/libraryStore.ts";

export type AlbumPreviewStatus = "idle" | "loading" | "ready" | "error";

export interface AlbumPreviewStore {
  /** The search item the page was opened from (title, artist and cover show while the tracks load). */
  item: SearchSuggestion | null;
  status: AlbumPreviewStatus;
  preview: AlbumPreview | null;
  error: string | null;
  /** Selected track indexes (positions in the album). */
  selection: SelectionState;

  /** Opens the page for an album/set search item and loads its tracks. */
  open: (item: SearchSuggestion) => Promise<void>;
  /** Loads the tracks again (after an error). */
  reload: () => Promise<void>;
  /** Back to the search results. */
  close: () => void;
  /** Click on a track row (Ctrl/Cmd adds, Shift selects a range). */
  selectTrack: (index: number, modifiers: ClickModifiers) => void;
  selectAll: () => void;
  clearSelection: () => void;
  /** Downloads the selected tracks, or the whole album when nothing (or everything) is selected. */
  download: () => Promise<void>;
}

const guard = createLatestGuard();

export const useAlbumPreviewStore = create<AlbumPreviewStore>((set, get) => ({
  item: null,
  status: "idle",
  preview: null,
  error: null,
  selection: EMPTY_SELECTION,

  open: async (item) => {
    const token = guard.begin();
    set({ item, status: "loading", preview: null, error: null, selection: EMPTY_SELECTION });
    useLibraryStore.getState().setActiveView("ALBUM_PREVIEW");
    try {
      const preview = await getAlbumPreview(item.platform, item.item_url_path);
      if (!guard.isLatest(token)) return;
      set({ preview, status: "ready" });
    } catch (e) {
      if (!guard.isLatest(token)) return;
      set({ status: "error", error: typeof e === "string" ? e : String(e) });
    }
  },

  reload: async () => {
    const { item } = get();
    if (item) await get().open(item);
  },

  close: () => {
    guard.begin(); // ignore a response that is still in flight
    useLibraryStore.getState().setActiveView("SEARCH_RESULTS");
  },

  selectTrack: (index, modifiers) => {
    const { preview, selection } = get();
    if (!preview) return;
    const selectable = selectableIndexes(preview);
    if (!selectable.includes(index)) return;
    set({ selection: nextSelection(selection, selectable, index, modifiers) });
  },

  selectAll: () => {
    const { preview, selection } = get();
    if (!preview) return;
    set({ selection: selectGroup(selection, selectableIndexes(preview), false) });
  },

  clearSelection: () => set({ selection: EMPTY_SELECTION }),

  download: async () => {
    const { preview, selection } = get();
    if (!preview) return;
    const available = selectableIndexes(preview);
    const urls = selectedUrls(preview, selection.selected);
    // Nothing selected, or everything: the plain album download (same folder and metadata).
    const wholeAlbum = urls.length === 0 || urls.length >= available.length;
    await useLibraryStore.getState().startDownload({
      platform: preview.platform,
      downloadType: ItemType.Album,
      url: preview.url,
      artistName: preview.artist,
      title: preview.title,
      coverUrl: preview.cover_url ?? undefined,
      trackUrls: wholeAlbum ? undefined : urls,
    });
  },
}));
