import { create } from "zustand";
import type { DownloadState } from "@/shared/lib/libraryIndex.ts";
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";

export interface RemoteTrackMenuOptions {
  /** The list the track belongs to (queue actions may use it). */
  list?: readonly RemoteTrack[];
  /** Known library state of the track (the menu shows "Downloaded" instead of "Download" when downloaded). */
  downloadState?: DownloadState;
}

export interface RemoteTrackMenuState {
  open: {
    position: { x: number; y: number };
    track: RemoteTrack;
    options: RemoteTrackMenuOptions;
  } | null;
  openMenu: (position: { x: number; y: number }, track: RemoteTrack, options?: RemoteTrackMenuOptions) => void;
  close: () => void;
}

/** State of the ONE remote-track context menu; `RemoteTrackMenuHost` (mounted once in App) renders it. */
export const useRemoteTrackMenuStore = create<RemoteTrackMenuState>((set) => ({
  open: null,
  openMenu: (position, track, options = {}) => set({ open: { position, track, options } }),
  close: () => set({ open: null }),
}));
