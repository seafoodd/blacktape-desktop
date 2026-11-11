import { create } from "zustand";
import { createLatestGuard } from "@/shared/lib/searchGuard.ts";
import {
  getAlbumPreview,
  selectableIndexes,
  selectableTracks,
  selectedUrls,
  type AlbumPreview,
  type AlbumTrackInfo,
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
  /**
   * Album downloads started from this page: library task id -> the track urls whose rows show its progress.
   * Entries are dropped when the library drops the task (see `startAlbumDownload`).
   */
  trackTasks: Record<string, string[]>;

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
  download: (format?: string) => Promise<void>;
  /** Downloads just these tracks (page urls) into the album folder, e.g. the ones the library lacks. */
  downloadUrls: (urls: string[], format?: string) => Promise<void>;
  /** Downloads one track into the album folder (album pipeline, so the album keeps its source row). */
  downloadTrack: (track: AlbumTrackInfo, format?: string) => Promise<void>;
}

const guard = createLatestGuard();

/** Keeps the entries whose task the library still knows (finished tasks leave `activeDownloads`). */
function pruneTasks(
  tasks: Record<string, string[]>,
  active: Record<string, unknown>,
): Record<string, string[]> {
  return Object.fromEntries(Object.entries(tasks).filter(([id]) => active[id] !== undefined));
}

/**
 * Starts an album-pipeline download (D13) and remembers which rows it serves. `startDownload` registers the task
 * in `activeDownloads` before its first await, so the one id that appears during the call belongs to this download.
 */
async function startAlbumDownload(
  preview: AlbumPreview,
  request: { trackUrls?: string[]; rows: string[]; format?: string },
): Promise<void> {
  const library = useLibraryStore.getState();
  const before = new Set(Object.keys(library.activeDownloads));
  const started = library.startDownload({
    platform: preview.platform,
    downloadType: ItemType.Album,
    url: preview.url,
    artistName: preview.artist,
    title: preview.title,
    coverUrl: preview.cover_url ?? undefined,
    trackUrls: request.trackUrls,
    format: request.format,
  });
  const active = useLibraryStore.getState().activeDownloads;
  const taskId = Object.keys(active).find((id) => !before.has(id));
  if (taskId !== undefined) {
    useAlbumPreviewStore.setState((s) => ({
      trackTasks: { ...pruneTasks(s.trackTasks, active), [taskId]: request.rows },
    }));
  }
  await started;
}

export const useAlbumPreviewStore = create<AlbumPreviewStore>((set, get) => ({
  item: null,
  status: "idle",
  preview: null,
  error: null,
  selection: EMPTY_SELECTION,
  trackTasks: {},

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

  download: async (format) => {
    const { preview, selection } = get();
    if (!preview) return;
    const available = selectableIndexes(preview);
    const urls = selectedUrls(preview, selection.selected);
    // Nothing selected, or everything: the plain album download (same folder and metadata).
    const wholeAlbum = urls.length === 0 || urls.length >= available.length;
    const rows = wholeAlbum ? selectableTracks(preview).map((t) => t.url) : urls;
    await startAlbumDownload(preview, { trackUrls: wholeAlbum ? undefined : urls, rows, format });
  },

  downloadUrls: async (urls, format) => {
    const { preview } = get();
    if (!preview || urls.length === 0) return;
    await startAlbumDownload(preview, { trackUrls: urls, rows: urls, format });
  },

  downloadTrack: async (track, format) => {
    const { preview } = get();
    if (!preview || !track.available || track.url === "") return;
    await startAlbumDownload(preview, { trackUrls: [track.url], rows: [track.url], format });
  },
}));
