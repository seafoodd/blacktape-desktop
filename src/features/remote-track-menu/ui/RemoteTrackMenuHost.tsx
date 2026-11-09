/**
 * Renders the one remote-track context menu from `useRemoteTrackMenuStore` (mounted once in App.tsx). The items
 * come from the pure `buildRemoteTrackMenu`; this file only wires the actions to the stores and the platform.
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTrackStatus } from "@/shared/hooks/useDownloadStatus.ts";
import { ContextMenu } from "@/shared/ui/context-menu";
import { useStreamStore } from "@/features/preview/model/streamStore.ts";
import { remoteTrackToStreamItem, type RemoteTrack } from "@/shared/lib/remoteTrack.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import {
  buildRemoteTrackMenu,
  remoteTrackDownloadParams,
  type RemoteTrackMenuActions,
} from "../model/buildRemoteTrackMenu.ts";
import {
  useRemoteTrackMenuStore,
  type RemoteTrackMenuOptions,
} from "../model/remoteTrackMenuStore.ts";

const actions: RemoteTrackMenuActions = {
  download: (track) => {
    useLibraryStore
      .getState()
      .startDownload(remoteTrackDownloadParams(track))
      .catch((e) => console.error("Download failed", e));
  },
  addToQueue: (track) => {
    void useStreamStore.getState().enqueue([remoteTrackToStreamItem(track)]);
  },
  playNext: (track) => {
    void useStreamStore.getState().enqueue([remoteTrackToStreamItem(track)], { playNext: true });
  },
  copyLink: (url) => {
    navigator.clipboard
      .writeText(url)
      .catch((e) => console.error("clipboard error: ", e));
  },
  openOnPlatform: (url) => {
    openUrl(url).catch((e) => console.error("openUrl error: ", e));
  },
};

interface OpenMenuProps {
  track: RemoteTrack;
  options: RemoteTrackMenuOptions;
  position: { x: number; y: number };
  onClose: () => void;
}

/** Mounted only while a menu is open, so the library status of the track is requested for that track only. */
const OpenMenu = ({ track, options, position, onClose }: OpenMenuProps) => {
  // Callers that already know the state (album rows) pass it; the rest get it from the library index.
  const libraryState = useTrackStatus({ platform: track.platform, url: track.url });
  const downloadState = options.downloadState ?? libraryState;
  return (
    <ContextMenu
      position={position}
      onClose={onClose}
      items={buildRemoteTrackMenu(track, { ...options, downloadState }, actions)}
    />
  );
};

export const RemoteTrackMenuHost = () => {
  const open = useRemoteTrackMenuStore((s) => s.open);
  const close = useRemoteTrackMenuStore((s) => s.close);
  if (!open) return null;
  return (
    <OpenMenu
      track={open.track}
      options={open.options}
      position={open.position}
      onClose={close}
    />
  );
};
