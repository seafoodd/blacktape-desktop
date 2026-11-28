import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { createLatestGuard } from "@/shared/lib/search/searchGuard.ts";
import {
  getAlbumPreview,
  selectableIndexes,
  selectableTracks,
  selectedUrls,
  snapshotMetadata,
  type AlbumPreview,
  type AlbumTrackInfo,
  type MetadataSnapshot,
} from "@/shared/lib/remote/albumPreview.ts";
import type { AudioCandidate, ReleaseEditionOption } from "@/shared/types/canonical.ts";
import type { Platform } from "@/shared/types.ts";
import { getReleaseGroupReleases } from "@/shared/lib/audio.ts";
import {
  EMPTY_SELECTION,
  nextSelection,
  selectGroup,
  type ClickModifiers,
  type SelectionState,
} from "@/shared/lib/ui/selection.ts";
import { ItemType, useLibraryStore, type DownloadOverrides, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { useNavigationStore } from "@/features/navigation/model/navigationStore.ts";

export type AlbumPreviewStatus = "idle" | "loading" | "ready" | "error";

export interface AlbumPreviewStore {
  /** The search item the page was opened from (title, artist and cover show while the tracks load). */
  item: SearchSuggestion | null;
  status: AlbumPreviewStatus;
  preview: AlbumPreview | null;
  originalMetadata: MetadataSnapshot | null;
  isEditingMetadata: boolean;
  error: string | null;
  /** True while background multi-source candidate resolution is still running for this release. */
  resolvingCandidates: boolean;
  /** Distinct releases in this MusicBrainz release-group, when available. */
  editions: ReleaseEditionOption[];
  loadingEditions: boolean;
  switchingRelease: boolean;
  /** Selected track indexes (positions in the album). */
  selection: SelectionState;
  /**
   * Album downloads started from this page: library task id -> the track urls whose rows show its progress.
   * Entries are dropped when the library drops the task (see `startAlbumDownload`).
   */
  trackTasks: Record<string, string[]>;

  /** Opens the page for an album/set search item and loads its tracks. */
  open: (item: SearchSuggestion) => Promise<void>;
  /** Switches to a different MusicBrainz edition or platform release in the picker. */
  selectRelease: (editionOrId: ReleaseEditionOption | string) => Promise<void>;
  /** Loads the tracks again (after an error). */
  reload: () => Promise<void>;
  /** Back to the search results. */
  close: () => void;
  setEditingMetadata: (editing: boolean) => void;
  updatePreviewMetadata: (patch: {
    title?: string;
    artist?: string;
    year?: number | null;
    trackIndex?: number;
    trackTitle?: string;
  }) => void;
  undoMetadataChanges: () => void;
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
  /** Cancels in-flight open/selectRelease responses when navigation switches views. */
  invalidatePending: () => void;
  /** Applies background-completed candidates/editions after restoring a snapshot. */
  resumeAfterRestore: () => Promise<void>;
}

const guard = createLatestGuard();

const resolvedCandidatesByUrl = new Map<
  string,
  { tracks: Record<number, AlbumTrackInfo>; total_duration_ms: number | null; done: boolean }
>();
const incompleteCandidateUrls = new Set<string>();

function hasNonTorrentCandidates(track: AlbumTrackInfo): boolean {
  return track.candidates?.some((candidate) => candidate.source.kind !== "torrent") ?? false;
}

function withResolvedCandidates(
  track: AlbumTrackInfo,
  candidates: NonNullable<AlbumTrackInfo["candidates"]>,
  selectedIndex: number,
) {
  const resolved = candidates.filter((candidate) => candidate.source.kind !== "torrent");
  const torrents = track.candidates?.filter((candidate) => candidate.source.kind === "torrent") ?? [];
  const selected = track.candidates?.[track.selected_candidate_idx ?? 0];
  const retainedIndex = selected ? torrents.indexOf(selected) : -1;
  return {
    candidates: [...resolved, ...torrents],
    selected_candidate_idx: retainedIndex < 0
      ? Math.max(0, resolved.indexOf(candidates[selectedIndex]))
      : resolved.length + retainedIndex,
  };
}

function mergeTracksFromCache(preview: AlbumPreview): { preview: AlbumPreview; cacheDone: boolean } {
  const cached = resolvedCandidatesByUrl.get(preview.url);
  if (!cached) return { preview, cacheDone: false };
  let changed = false;
  const tracks = preview.tracks.map((tr) => {
    const ct = cached.tracks[tr.index];
    if (!ct) return tr;
    const hasFreshCands = hasNonTorrentCandidates(ct);
    const trMissingCands = !hasNonTorrentCandidates(tr);
    if (!hasFreshCands && tr.duration_ms === ct.duration_ms) return tr;
    changed = true;
    return {
      ...tr,
      ...(trMissingCands && hasFreshCands && ct.candidates
        ? withResolvedCandidates(tr, ct.candidates, ct.selected_candidate_idx ?? 0)
        : {}),
      url: trMissingCands && hasFreshCands ? ct.url || tr.url : tr.url,
      duration_ms: tr.duration_ms && tr.duration_ms > 0 ? tr.duration_ms : ct.duration_ms,
    };
  });
  if (!changed) return { preview, cacheDone: cached.done };
  const totalSum = tracks.reduce(
    (sum, tr) => sum + (typeof tr.duration_ms === "number" && tr.duration_ms > 0 ? tr.duration_ms : 0),
    0,
  );
  return {
    preview: {
      ...preview,
      tracks,
      total_duration_ms: totalSum > 0 ? totalSum : cached.total_duration_ms ?? preview.total_duration_ms,
    },
    cacheDone: cached.done,
  };
}

function shouldResolveCandidates(preview: AlbumPreview, cacheDone: boolean): boolean {
  if (incompleteCandidateUrls.has(preview.url)) return true;
  if (preview.candidates_resolved || cacheDone) return false;
  return preview.tracks.some((t) => t.available && (!t.candidates || t.candidates.length === 0));
}

/** Keeps the entries whose task the library still knows (finished tasks leave `activeDownloads`). */
function pruneTasks(
  tasks: Record<string, string[]>,
  active: Record<string, unknown>,
): Record<string, string[]> {
  return Object.fromEntries(Object.entries(tasks).filter(([id]) => active[id] !== undefined));
}

/** Builds backend `DownloadOverrides` from staged `MetadataEditModal` edits and `CandidateSelector` picks. Pure. */
export function buildDownloadOverrides(preview: AlbumPreview): DownloadOverrides {
  return {
    title: preview.title,
    artist: preview.artist,
    year: preview.year ?? null,
    tracks: preview.tracks.map((track) => {
      const cands = track.candidates ?? [];
      const chosenIdx =
        typeof track.selected_candidate_idx === "number" && track.selected_candidate_idx < cands.length
          ? track.selected_candidate_idx
          : 0;
      const chosen = cands[chosenIdx];
      const chosenUrl =
        chosen?.source.kind === "http_stream" ? chosen.source.url : track.url;
      const fallbacks = cands
        .filter((_, idx) => idx !== chosenIdx)
        .map((c) => (c.source.kind === "http_stream" ? c.source.url : ""))
        .filter((u) => u !== "" && u !== chosenUrl);
      return {
        index: track.index,
        title: track.title,
        original_url: track.url,
        url: chosenUrl || track.url,
        fallback_urls: fallbacks,
        candidates: cands.length > 0 ? cands : undefined,
        selected_candidate_idx: cands.length > 0 ? chosenIdx : undefined,
      };
    }),
  };
}

/**
 * Starts an album-pipeline download (D13) and remembers which rows it serves. `startDownload` registers the task
 * in `activeDownloads` before its first await, so the one id that appears during the call belongs to this download.
 */
async function startAlbumDownload(
  preview: AlbumPreview,
  request: { trackUrls?: string[]; rows: string[]; format?: string },
): Promise<void> {
  const library = useDownloadStore.getState();
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
    overrides: buildDownloadOverrides(preview),
  });
  const active = useDownloadStore.getState().activeDownloads;
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
  originalMetadata: null,
  isEditingMetadata: false,
  error: null,
  resolvingCandidates: false,
  editions: [],
  loadingEditions: false,
  switchingRelease: false,
  selection: EMPTY_SELECTION,
  trackTasks: {},

  open: async (item) => {
    const token = guard.begin();
    set({
      item,
      status: "loading",
      preview: null,
      originalMetadata: null,
      isEditingMetadata: false,
      error: null,
      resolvingCandidates: false,
      editions: [],
      loadingEditions: false,
      switchingRelease: false,
      selection: EMPTY_SELECTION,
    });
    useLibraryStore.getState().setActiveView("ALBUM_PREVIEW");
    try {
      const rawPreview = await getAlbumPreview(item.platform, item.item_url_path, undefined, {
        artist: item.band_name,
        title: item.name,
        coverUrl: item.img,
      });
      if (!guard.isLatest(token)) return;
      const { preview, cacheDone } = mergeTracksFromCache(rawPreview);
      const needsResolution = shouldResolveCandidates(preview, cacheDone);
      const snapshotData = {
        item,
        preview,
        originalMetadata: snapshotMetadata(preview),
        editions: [] as ReleaseEditionOption[],
        resolvingCandidates: needsResolution,
        isEditingMetadata: false,
      };
      set({
        preview: snapshotData.preview,
        originalMetadata: snapshotData.originalMetadata,
        status: "ready",
        resolvingCandidates: needsResolution,
      });
      useNavigationStore.getState().syncAlbumSnapshotByItem?.(item.item_url_path, snapshotData);
      const rgId = preview.release_group_id;
      if ((rgId && preview.metadata_source === "musicbrainz") || (preview.artist && preview.title)) {
        set({ loadingEditions: true });
        Promise.resolve(getReleaseGroupReleases(rgId ?? null, preview.artist, preview.title))
          .then((editions) => {
            if (!guard.isLatest(token)) return;
            const curState = get();
            const curPreview =
              curState.preview?.url === preview.url ? curState.preview : snapshotData.preview;
            const curResolving =
              curState.preview?.url === preview.url ? curState.resolvingCandidates : needsResolution;
            set({ editions, loadingEditions: false });
            useNavigationStore.getState().syncAlbumSnapshotByItem?.(item.item_url_path, {
              ...snapshotData,
              preview: curPreview,
              resolvingCandidates: curResolving,
              editions,
            });
          })
          .catch(() => {
            if (!guard.isLatest(token)) return;
            set({ loadingEditions: false });
          });
      }
    } catch (e) {
      if (!guard.isLatest(token)) return;
      set({ status: "error", error: typeof e === "string" ? e : String(e), resolvingCandidates: false });
    }
  },

  selectRelease: async (editionOrId) => {
    const { preview, editions } = get();
    if (!preview) return;
    const edition =
      typeof editionOrId === "string"
        ? editions.find((e) => e.id === editionOrId) ?? {
            id: editionOrId,
            source_kind: "musicbrainz",
            url: `canonical:${editionOrId}`,
          }
        : editionOrId;

    const isPlatformRelease =
      Boolean(edition.source_kind) && edition.source_kind !== "musicbrainz" && Boolean(edition.url);
    const targetUrl = isPlatformRelease ? edition.url! : `canonical:${edition.id}`;
    const targetPlatform = isPlatformRelease
      ? (edition.source_kind as Platform)
      : preview.platform;

    if (
      (edition.id && preview.selected_release_id === edition.id) ||
      (isPlatformRelease && preview.url === `canonical:${targetUrl}`)
    ) {
      return;
    }

    const token = guard.begin();
    set({
      switchingRelease: true,
      loadingEditions: false,
      error: null,
      isEditingMetadata: false,
      selection: EMPTY_SELECTION,
    });
    try {
      const rawNextPreview = await getAlbumPreview(
        targetPlatform,
        targetUrl,
        isPlatformRelease,
      );
      if (!guard.isLatest(token)) return;
      const { preview: mergedNext, cacheDone } = mergeTracksFromCache(rawNextPreview);
      const needsResolution = shouldResolveCandidates(mergedNext, cacheDone);
      const nextPreview: AlbumPreview = {
        ...mergedNext,
        cover_url: mergedNext.cover_url ?? preview.cover_url,
        release_group_id: mergedNext.release_group_id ?? preview.release_group_id ?? null,
        selected_release_id: isPlatformRelease
          ? edition.id
          : mergedNext.selected_release_id ?? edition.id,
      };
      const nextOriginalMetadata = snapshotMetadata(mergedNext);
      set({
        preview: nextPreview,
        originalMetadata: nextOriginalMetadata,
        status: "ready",
        resolvingCandidates: needsResolution,
        switchingRelease: false,
      });
      const currentItem = get().item;
      if (currentItem) {
        useNavigationStore.getState().syncAlbumSnapshotByItem?.(currentItem.item_url_path, {
          item: currentItem,
          preview: nextPreview,
          originalMetadata: nextOriginalMetadata,
          editions: get().editions,
          resolvingCandidates: needsResolution,
          isEditingMetadata: false,
        });
      }
    } catch (e) {
      if (!guard.isLatest(token)) return;
      set({
        switchingRelease: false,
        error: typeof e === "string" ? e : String(e),
      });
    }
  },

  reload: async () => {
    const { item } = get();
    if (item) await get().open(item);
  },

  close: () => {
    guard.begin(); // ignore a response that is still in flight
    set({ isEditingMetadata: false });
    const nav = useNavigationStore.getState();
    if (nav.canGoBack()) {
      void nav.goBack();
    } else {
      useLibraryStore.getState().setActiveView("SEARCH_RESULTS");
    }
  },

  setEditingMetadata: (editing) => set({ isEditingMetadata: editing }),

  updatePreviewMetadata: (patch) => {
    const { preview } = get();
    if (!preview) return;
    const nextTracks =
      patch.trackIndex !== undefined && patch.trackTitle !== undefined
        ? preview.tracks.map((t) =>
            t.index === patch.trackIndex ? { ...t, title: patch.trackTitle! } : t,
          )
        : preview.tracks;
    set({
      preview: {
        ...preview,
        title: patch.title !== undefined ? patch.title : preview.title,
        artist: patch.artist !== undefined ? patch.artist : preview.artist,
        year: patch.year !== undefined ? patch.year : preview.year,
        tracks: nextTracks,
      },
    });
  },

  undoMetadataChanges: () => {
    const { preview, originalMetadata } = get();
    if (!preview || !originalMetadata) return;
    set({
      preview: {
        ...preview,
        title: originalMetadata.title,
        artist: originalMetadata.artist,
        year: originalMetadata.year,
        tracks: preview.tracks.map((t) => ({
          ...t,
          title: originalMetadata.trackTitles[t.index] ?? t.title,
        })),
      },
    });
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

  invalidatePending: () => {
    guard.begin();
    set({ loadingEditions: false, switchingRelease: false });
  },

  resumeAfterRestore: async () => {
    const { preview, editions, resolvingCandidates, status } = get();
    if (status !== "ready" || !preview) return;

    const { preview: mergedFromCache, cacheDone } = mergeTracksFromCache(preview);
    const nextResolving = incompleteCandidateUrls.has(preview.url) || (cacheDone || mergedFromCache.candidates_resolved ? false : resolvingCandidates);
    if (mergedFromCache !== preview || nextResolving !== resolvingCandidates) {
      set({ preview: mergedFromCache, resolvingCandidates: nextResolving });
    }

    const targetUrl = mergedFromCache.url;
    const rgId = mergedFromCache.release_group_id;
    const needsEditions =
      editions.length === 0 &&
      ((Boolean(rgId) && mergedFromCache.metadata_source === "musicbrainz") ||
        (Boolean(mergedFromCache.artist) && Boolean(mergedFromCache.title)));

    if (!needsEditions && !nextResolving) return;
    const token = guard.begin();

    if (needsEditions) {
      set({ loadingEditions: true });
      Promise.resolve(getReleaseGroupReleases(rgId ?? null, mergedFromCache.artist, mergedFromCache.title))
        .then((nextEditions) => {
          if (!guard.isLatest(token)) return;
          set({ editions: nextEditions, loadingEditions: false });
          const curState = get();
          const curItem = curState.item;
          const curPreview = curState.preview;
          if (curItem && curPreview && curPreview.url === targetUrl) {
            useNavigationStore.getState().syncAlbumSnapshotByItem?.(curItem.item_url_path, {
              item: curItem,
              preview: curPreview,
              originalMetadata: curState.originalMetadata,
              editions: nextEditions,
              resolvingCandidates: curState.resolvingCandidates,
              isEditingMetadata: curState.isEditingMetadata,
            });
          }
        })
        .catch(() => {
          if (!guard.isLatest(token)) return;
          set({ loadingEditions: false });
        });
    }

    if (nextResolving) {
      const isPlatformRelease =
        Boolean(mergedFromCache.metadata_source) &&
        mergedFromCache.metadata_source !== "musicbrainz" &&
        !targetUrl.startsWith("canonical:");
      try {
        const fresh = await getAlbumPreview(mergedFromCache.platform, targetUrl, isPlatformRelease);
        if (!guard.isLatest(token)) return;
        const cur = get();
        if (!cur.preview || cur.preview.url !== targetUrl) return;

        const mergedTracks = cur.preview.tracks.map((tr, idx) => {
          const ft = fresh.tracks.find((x) => x.index === tr.index) ?? fresh.tracks[idx];
          if (!ft) return tr;
          const trMissingCands = !hasNonTorrentCandidates(tr);
          const freshHasCands = hasNonTorrentCandidates(ft);
          if (!trMissingCands && (tr.duration_ms ?? 0) > 0) return tr;
          return {
            ...tr,
            ...(trMissingCands && freshHasCands && ft.candidates
              ? withResolvedCandidates(tr, ft.candidates, ft.selected_candidate_idx ?? 0)
              : {}),
            url: trMissingCands && freshHasCands ? ft.url || tr.url : tr.url,
            duration_ms: tr.duration_ms && tr.duration_ms > 0 ? tr.duration_ms : ft.duration_ms,
          };
        });
        const totalSum = mergedTracks.reduce(
          (sum, tr) => sum + (typeof tr.duration_ms === "number" && tr.duration_ms > 0 ? tr.duration_ms : 0),
          0,
        );
        const updatedPreview: AlbumPreview = {
          ...cur.preview,
          tracks: mergedTracks,
          total_duration_ms: totalSum > 0 ? totalSum : cur.preview.total_duration_ms,
          candidates_resolved: fresh.candidates_resolved,
        };
        const doneNow =
          Boolean(fresh.candidates_resolved) ||
          Boolean(resolvedCandidatesByUrl.get(targetUrl)?.done);
        if (doneNow) {
          const entry = resolvedCandidatesByUrl.get(targetUrl) ?? {
            tracks: {},
            total_duration_ms: updatedPreview.total_duration_ms ?? null,
            done: true,
          };
          for (const tr of mergedTracks) entry.tracks[tr.index] = tr;
          entry.total_duration_ms = updatedPreview.total_duration_ms ?? null;
          entry.done = true;
          resolvedCandidatesByUrl.set(targetUrl, entry);
          useNavigationStore.getState().syncAlbumCandidatesDone?.(targetUrl);
        }
        set({ preview: updatedPreview, resolvingCandidates: !doneNow });
      } catch {
        if (!guard.isLatest(token)) return;
        set({ resolvingCandidates: false });
      }
    }
  },
}));

export interface AlbumTrackCandidatesEvent {
  album_url: string;
  track_index: number;
  candidates: AudioCandidate[];
  selected_candidate_idx: number;
  duration_ms?: number | null;
}

function applyCandidateEventToPreview(
  preview: AlbumPreview,
  payload: AlbumTrackCandidatesEvent,
): { nextPreview: AlbumPreview; resolvingCandidates: boolean; updatedTrack: AlbumTrackInfo } | null {
  const tracks = [...preview.tracks];
  const t = tracks[payload.track_index];
  if (!t) return null;
  const bestCandidate = payload.candidates[0];
  const resolvedDurationMs =
    payload.duration_ms && payload.duration_ms > 0
      ? payload.duration_ms
      : bestCandidate && bestCandidate.duration_ms > 0
        ? bestCandidate.duration_ms
        : null;
  const nextDurationMs =
    t.duration_ms && t.duration_ms > 0 ? t.duration_ms : resolvedDurationMs ?? t.duration_ms;

  const updatedTrack: AlbumTrackInfo = {
    ...t,
    ...withResolvedCandidates(t, payload.candidates, payload.selected_candidate_idx),
    duration_ms: nextDurationMs,
    url:
      bestCandidate && bestCandidate.source.kind === "http_stream"
        ? bestCandidate.source.url
        : t.url,
  };
  tracks[payload.track_index] = updatedTrack;
  const totalSum = tracks.reduce(
    (sum, tr) => sum + (typeof tr.duration_ms === "number" && tr.duration_ms > 0 ? tr.duration_ms : 0),
    0,
  );
  const nextPreview: AlbumPreview = {
    ...preview,
    tracks,
    total_duration_ms: totalSum > 0 ? totalSum : preview.total_duration_ms,
  };
  const resolvingCandidates = nextPreview.tracks.some(
    (tr) => tr.available && !hasNonTorrentCandidates(tr),
  );
  return { nextPreview, resolvingCandidates, updatedTrack };
}

if (typeof window !== "undefined") {
  listen<AlbumTrackCandidatesEvent>("album-track-candidates", (event) => {
    const { album_url, track_index } = event.payload;
    const cached = resolvedCandidatesByUrl.get(album_url) ?? {
      tracks: {},
      total_duration_ms: null,
      done: false,
    };
    const existingCached = cached.tracks[track_index];
    const bestCandidate = event.payload.candidates[0];
    const resolvedDurationMs =
      event.payload.duration_ms && event.payload.duration_ms > 0
        ? event.payload.duration_ms
        : bestCandidate && bestCandidate.duration_ms > 0
          ? bestCandidate.duration_ms
          : null;
    cached.tracks[track_index] = {
      ...(existingCached ?? {
        index: track_index,
        title: "",
        artists: [],
        track_number: null,
        url: "",
        available: true,
        note: null,
      }),
      candidates: event.payload.candidates,
      selected_candidate_idx: event.payload.selected_candidate_idx,
      duration_ms:
        existingCached?.duration_ms && existingCached.duration_ms > 0
          ? existingCached.duration_ms
          : resolvedDurationMs ?? existingCached?.duration_ms,
      url:
        bestCandidate && bestCandidate.source.kind === "http_stream"
          ? bestCandidate.source.url
          : existingCached?.url ?? "",
    };
    resolvedCandidatesByUrl.set(album_url, cached);

    const { preview } = useAlbumPreviewStore.getState();
    if (preview && preview.url === album_url) {
      const applied = applyCandidateEventToPreview(preview, event.payload);
      if (applied) {
        useAlbumPreviewStore.setState({
          preview: applied.nextPreview,
          resolvingCandidates: applied.resolvingCandidates,
        });
      }
    }

    useNavigationStore.getState().syncAlbumCandidates?.(album_url, (snapPreview) => {
      const applied = applyCandidateEventToPreview(snapPreview, event.payload);
      return applied
        ? { preview: applied.nextPreview, resolvingCandidates: applied.resolvingCandidates }
        : null;
    });
  })?.catch(console.error);

  listen<{ album_url: string; complete?: boolean }>("album-candidates-done", (event) => {
    const albumUrl = event.payload.album_url;
    if (event.payload.complete === false) {
      incompleteCandidateUrls.add(albumUrl);
      resolvedCandidatesByUrl.delete(albumUrl);
      const { preview } = useAlbumPreviewStore.getState();
      if (preview?.url === albumUrl) {
        useAlbumPreviewStore.setState({ preview: { ...preview, candidates_resolved: false }, resolvingCandidates: false });
      }
      useNavigationStore.getState().syncAlbumCandidates?.(albumUrl, (snapshot) => ({
        preview: { ...snapshot, candidates_resolved: false }, resolvingCandidates: false,
      }));
      return;
    }
    incompleteCandidateUrls.delete(albumUrl);
    const cached = resolvedCandidatesByUrl.get(albumUrl) ?? {
      tracks: {},
      total_duration_ms: null,
      done: true,
    };
    cached.done = true;
    resolvedCandidatesByUrl.set(albumUrl, cached);

    const { preview } = useAlbumPreviewStore.getState();
    if (preview && preview.url === albumUrl) {
      useAlbumPreviewStore.setState({ resolvingCandidates: false });
    }
    useNavigationStore.getState().syncAlbumCandidatesDone?.(albumUrl);
  })?.catch(console.error);
}
