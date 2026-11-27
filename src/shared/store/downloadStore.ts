import { create } from "zustand";
import { reduceDownloadEvent, type DownloadEvent } from "@/shared/store/downloadEvents.ts";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getArtistAlbums } from "@/shared/lib/audio.ts";
import { type Platform, type Song } from "@/shared/types.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { findDownload, isRunning } from "@/shared/lib/library/downloadMatch.ts";
import { captureFrontendError, isCancellationError } from "@/shared/lib/telemetry.ts";

export type DownloadStatus =
  | "idle"
  | "downloading"
  | "processing"
  | "completed"
  | "failed"
  | "cancelled";

export interface TrackDownloadProgress {
  title: string;
  percent: number;
  status: "pending" | "downloading" | "completed" | "failed";
  trackNumber?: number;
  discNumber?: number;
}

interface DownloadOutcomeEvent {
  task_id: string;
  message: string;
  tracks?: {
    title: string;
    percent: number;
    status: TrackDownloadProgress["status"];
    track_number?: number;
    disc_number?: number;
  }[];
}

export interface DownloadProgress {
  cancelling?: boolean;
  cancelError?: string;
  id: string;
  url: string;
  status: DownloadStatus;
  current: number;
  total: number;
  message: string;
  artistName: string;
  albumTitle: string;
  coverUrl?: string;
  releaseYear?: number;
  itemType: ItemType | "Track" | "Album";
  platform: Platform;
  overallPercent: number;
  tracks: TrackDownloadProgress[];
  /** A single track filed into a library song: its progress belongs to that song, whatever title the platform gave it. */
  placement?: TrackPlacement;
  /** Album downloads of some tracks: their page urls (absent for a whole album and for recovered downloads). */
  trackUrls?: string[];
  /** Library song id when upgrading/replacing a single existing track. */
  replaceSongId?: number;
}

interface TrackOverride {
  index?: number;
  original_url?: string;
  title?: string;
  url?: string;
  fallback_urls?: string[];
  candidates?: AudioCandidate[];
  selected_candidate_idx?: number;
}

export interface DownloadOverrides {
  title?: string;
  artist?: string;
  year?: number | null;
  tracks?: TrackOverride[];
}

export interface StartDownloadParams {
  platform: Platform | string;
  downloadType: ItemType | string;
  url: string;
  artistName?: string;
  title?: string;
  coverUrl?: string;
  /** Album downloads only: page urls of the tracks to download (all tracks when omitted). */
  trackUrls?: string[];
  /** Single track downloads: the library album and song this track stands for (names and number win over the platform's). */
  placement?: TrackPlacement;
  /** yt-dlp format id picked in the quality menu (the platform's best is the fallback); omitted: global default. */
  format?: string;
  /** Edited album/track metadata & chosen candidates staged in AlbumPreview. */
  overrides?: DownloadOverrides;
  /** Library song id to atomically upgrade and replace (old file moves to Trash after verified download). */
  replaceSongId?: number;
}

/** Mirrors the Rust `TrackPlacement` (snake_case on the wire). */
export interface TrackPlacement {
  album: string;
  album_artist: string;
  title: string;
  track_number: number | null;
}

interface DownloadState {
  activeDownloads: Record<string, DownloadProgress>;
  _listenersInitialized: boolean;
  initDownloadListeners: () => Promise<void>;
  startDownload: (
    paramsOrPlatform: StartDownloadParams | string,
    downloadType?: string,
    url?: string,
  ) => Promise<void>;
  cancelDownload: (taskId: string) => Promise<void>;
  dismissDownload: (taskId: string) => void;
}

export const useDownloadStore = create<DownloadState>((set, get) => {
  const applyEvent = (event: DownloadEvent) => set((state) => ({
    activeDownloads: reduceDownloadEvent(state.activeDownloads, event),
  }));

  return {
    activeDownloads: {},
    _listenersInitialized: false,

    initDownloadListeners: async () => {
      if (get()._listenersInitialized) return;
      set({ _listenersInitialized: true });

      const pendingEvents: (() => void)[] = [];
      const unlisten: (() => void)[] = [];
      let recovering = true;
      const subscribe = async <T>(name: string, handler: (event: { payload: T }) => void | Promise<void>) => {
        unlisten.push(await listen<T>(name, (event) => {
          if (recovering) {
            pendingEvents.push(() => { void handler(event); });
            return;
          }
          return handler(event);
        }));
      };

      try {
        await subscribe<{ task_id: string; message: string }>(
          "download-task-started",
          (event) => {
            applyEvent({ type: "started", ...event.payload });
          },
        );

        await subscribe<{
          task_id: string;
          title: string;
          artist: string;
          cover_url?: string;
          release_year?: number;
          tracks: { title: string; track_number?: number; disc_number?: number }[];
        }>("download-album-meta", (event) => {
          applyEvent({ type: "metadata", ...event.payload });
        });

        await subscribe<{
          task_id: string;
          track_index: number;
          track_title: string;
          percent: number;
        }>("download-track-progress", (event) => {
          applyEvent({ type: "track-progress", ...event.payload });
        });

        let trackSavedDebounceTimer: ReturnType<typeof setTimeout> | null = null;
        await subscribe<{
          task_id: string;
          track_index: number;
          song: Song;
        }>("download-track-saved", (event) => {
          const { song } = event.payload;

          applyEvent({ type: "saved", ...event.payload });

          if (song?.source_url) {
            invoke("swap_streams_for_downloads", { songs: [song] }).catch((e) =>
              console.error("swap_streams_for_downloads per-track error:", e),
            );
          }

          if (trackSavedDebounceTimer) {
            clearTimeout(trackSavedDebounceTimer);
          }
          trackSavedDebounceTimer = setTimeout(async () => {
            trackSavedDebounceTimer = null;
            useLibraryStore.getState().fetchTabs()
              .catch((e) => console.error("Auto-fetch tabs on track saved failed", e));

            const { selectedTab } = useLibraryStore.getState();
            if (selectedTab) {
              try {
                const albums = await getArtistAlbums(selectedTab);
                useLibraryStore.setState({ albums });
              } catch (e) {
                console.error("Auto-fetch albums on track saved failed", e);
              }
            }
          }, 150);
        });

        await subscribe<{
          task_id: string;
          current: number;
          total: number;
          track_title: string;
        }>("download-task-progress", (event) => {
          applyEvent({ type: "progress", ...event.payload });
        });

        await subscribe<DownloadOutcomeEvent>(
          "download-task-completed",
          async (event) => {
            const taskId = event.payload.task_id;
            const downloadedArtist = get().activeDownloads[taskId]?.artistName;
            applyEvent({ type: "completed", ...event.payload });

            useLibraryStore.getState().fetchTabs()
              .catch((e) => console.error("Auto-fetch failed", taskId, e));

            const { selectedTab } = useLibraryStore.getState();
            if (selectedTab) {
              try {
                const albums = await getArtistAlbums(selectedTab);
                useLibraryStore.setState({ albums });
              } catch (e) {
                console.error("Auto-fetch albums failed", taskId, e);
              }
            }

            // Streams of the songs that were just downloaded (playing or queued) switch to the files.
            if (downloadedArtist) {
              try {
                const songs = (await getArtistAlbums(downloadedArtist))
                  .flatMap((album) => album.songs)
                  .filter((song) => !!song.source_url);
                if (songs.length > 0) await invoke("swap_streams_for_downloads", { songs });
              } catch (e) {
                console.error("swap_streams_for_downloads failed", taskId, e);
              }
            }

            setTimeout(() => {
              set((state) => {
                const active = { ...state.activeDownloads };
                if (active[taskId]?.status === "completed") {
                  delete active[taskId];
                  return { activeDownloads: active };
                }
                return {};
              });
            }, 2500);
          },
        );

        await subscribe<DownloadOutcomeEvent>(
          "download-task-failed",
          (event) => {
            applyEvent({ type: "failed", ...event.payload });
          },
        );

        await subscribe<{ task_id: string }>("download-task-cancelled", (event) => {
          applyEvent({ type: "cancelled", ...event.payload });
        });
        try {
          const activeList = await invoke<
            {
              id: string;
              platform: string;
              download_type: string;
              url: string;
              artist_name: string;
              album_title: string;
              cover_url?: string;
              release_year?: number;
              status: string;
              current: number;
              total: number;
              overall_percent: number;
              message: string;
              tracks: {
                title: string;
                track_number?: number;
                disc_number?: number;
                percent: number;
                status: string;
              }[];
            }[]
          >("get_active_downloads");

          if (activeList && activeList.length > 0) {
            set((state) => {
              const activeDownloads = { ...state.activeDownloads };
              for (const item of activeList) {
                activeDownloads[item.id] = {
                  ...activeDownloads[item.id],
                  id: item.id,
                  url: item.url,
                  status: item.status as DownloadStatus,
                  current: item.current,
                  total: item.total,
                  message: item.message,
                  artistName: item.artist_name,
                  albumTitle: item.album_title,
                  coverUrl: item.cover_url,
                  releaseYear: item.release_year,
                  itemType: (item.download_type === "Track"
                    ? ItemType.Track
                    : ItemType.Album) as ItemType,
                  platform: item.platform as Platform,
                  overallPercent: item.overall_percent,
                  tracks: (item.tracks || []).map((t) => ({
                    title: t.title,
                    trackNumber: t.track_number,
                    discNumber: t.disc_number,
                    percent: t.percent,
                    status: t.status as TrackDownloadProgress["status"],
                  })),
                };
              }
              return { activeDownloads };
            });
          }
        } catch (e) {
          console.error("Failed to recover active downloads on load:", e);
        }

        recovering = false;
        for (const apply of pendingEvents.splice(0)) apply();
      } catch (error) {
        unlisten.forEach((stop) => stop());
        set({ _listenersInitialized: false });
        throw error;
      }

    },

    startDownload: async (paramsOrPlatform, downloadTypeArg, urlArg) => {
      const isObject = typeof paramsOrPlatform === "object";
      const platform = isObject ? paramsOrPlatform.platform : paramsOrPlatform;
      const downloadType = isObject
        ? paramsOrPlatform.downloadType
        : downloadTypeArg!;
      const url = isObject ? paramsOrPlatform.url : urlArg!;
      const artistName = isObject
        ? paramsOrPlatform.artistName || "Unknown Artist"
        : "Unknown Artist";
      const title = isObject ? paramsOrPlatform.title || "" : "";
      const coverUrl = isObject ? paramsOrPlatform.coverUrl : undefined;
      const trackUrls = isObject ? paramsOrPlatform.trackUrls : undefined;
      const placement = isObject ? paramsOrPlatform.placement : undefined;
      const format = isObject ? paramsOrPlatform.format : undefined;
      const overrides = isObject ? paramsOrPlatform.overrides : undefined;
      const replaceSongId = isObject ? paramsOrPlatform.replaceSongId : undefined;

      // A second click while the same songs are still downloading would queue them again.
      if (isRunning(findDownload(get().activeDownloads, { url, trackUrls }))) return;

      const taskId = crypto.randomUUID();
      const isSingleTrack =
        downloadType === ItemType.Track || downloadType === "Track";

      set((state) => ({
        activeDownloads: {
          ...state.activeDownloads,
          [taskId]: {
            id: taskId,
            url,
            status: "downloading",
            current: 0,
            total: isSingleTrack ? 1 : (trackUrls?.length ?? 0),
            message: "Starting download...",
            artistName,
            // A single track has no album title until its page was read (then "Singles" or its album).
            albumTitle: placement ? placement.album : isSingleTrack ? "" : title || "Album",
            coverUrl,
            itemType: downloadType as ItemType,
            platform: platform as Platform,
            overallPercent: 0,
            placement,
            trackUrls,
            replaceSongId,
            tracks: isSingleTrack
              ? [
                  {
                    title: title || "Track",
                    percent: 0,
                    status: "downloading",
                    trackNumber: placement?.track_number ?? 1,
                  },
                ]
              : [],
          },
        },
      }));

      try {
        await invoke("download", {
          id: taskId,
          platform,
          downloadType,
          url,
          artistName,
          title,
          coverUrl,
          trackUrls: trackUrls ?? null,
          placement: placement ?? null,
          format: format ?? null,
          overrides: overrides ?? null,
          replaceSongId: replaceSongId ?? null,
        });
      } catch (e) {
        console.error("Download invoke failed:", e);
        if (!isCancellationError(e)) {
          captureFrontendError(e, "libraryStore.startDownload");
        }
        set((state) => {
          const existing = state.activeDownloads[taskId];
          if (!existing || existing.status === "completed" || existing.status === "failed") return {};
          return {
            activeDownloads: {
              ...state.activeDownloads,
              [taskId]: {
                ...existing,
                status: "failed",
                message: String(e),
              },
            },
          };
        });
      }
    },

    cancelDownload: async (taskId: string) => {
      const task = get().activeDownloads[taskId];
      if (!task || task.cancelling) return;
      set((state) => ({ activeDownloads: {
        ...state.activeDownloads,
        [taskId]: { ...task, cancelling: true, cancelError: undefined },
      } }));
      try {
        await invoke("cancel_download", { taskId });
      } catch (e) {
        console.error("Cancel download invoke failed:", e);
        captureFrontendError(e, "libraryStore.cancelDownload");
        set((state) => {
          const existing = state.activeDownloads[taskId];
          if (!existing) return {};
          return { activeDownloads: {
            ...state.activeDownloads,
            [taskId]: { ...existing, cancelling: false, cancelError: String(e) },
          } };
        });
        throw e;
      }
      set((state) => {
        const activeDownloads = { ...state.activeDownloads };
        delete activeDownloads[taskId];
        return { activeDownloads };
      });
    },

    dismissDownload: (taskId: string) => {
      set((state) => {
        if (!state.activeDownloads[taskId]) return {};
        const activeDownloads = { ...state.activeDownloads };
        delete activeDownloads[taskId];
        return { activeDownloads };
      });
    },
  };
});
