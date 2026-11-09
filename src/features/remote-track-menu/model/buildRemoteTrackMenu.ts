/**
 * Pure part of the remote-track context menu: which items a track gets and what each item asks for. The React
 * host (`RemoteTrackMenuHost`) supplies the actions, so this file has no side effects and is tested directly.
 */
import type { MenuItemOption } from "@/shared/ui/context-menu/ContextMenu.tsx";
import { platformLabel } from "@/shared/lib/platforms.ts";
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";
import { ItemType, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import type { RemoteTrackMenuOptions } from "./remoteTrackMenuStore.ts";

export interface RemoteTrackMenuActions {
  /** Starts the download of the track; the host passes `remoteTrackDownloadParams(track)` to `startDownload`. */
  download: (track: RemoteTrack) => void;
  addToQueue: (track: RemoteTrack) => void;
  playNext: (track: RemoteTrack) => void;
  copyLink: (url: string) => void;
  openOnPlatform: (url: string) => void;
}

/**
 * Parameters of "Download" for a track. A known album url sends the track through the album pipeline (D13), so it
 * lands in the album folder with the album metadata and the album gets its source row; otherwise it is a single
 * track download.
 */
export function remoteTrackDownloadParams(track: RemoteTrack): StartDownloadParams {
  const coverUrl = track.cover ?? undefined;
  if (track.album_url) {
    return {
      platform: track.platform,
      downloadType: ItemType.Album,
      url: track.album_url,
      trackUrls: [track.url],
      artistName: track.artist,
      title: track.album || track.title,
      coverUrl,
    };
  }
  return {
    platform: track.platform,
    downloadType: ItemType.Track,
    url: track.url,
    artistName: track.artist,
    title: track.title,
    coverUrl,
  };
}

/**
 * Items of the context menu of a remote track: Download (or "Downloaded", which has no action), Add to queue,
 * Play next, a separator, Copy link, Open on <platform>. `options.list` is not needed for these items.
 */
export function buildRemoteTrackMenu(
  track: RemoteTrack,
  options: RemoteTrackMenuOptions,
  actions: RemoteTrackMenuActions,
): MenuItemOption[] {
  const downloaded = options.downloadState === "downloaded";
  return [
    downloaded
      ? { label: "Downloaded", disabled: true }
      : { label: "Download", action: () => actions.download(track) },
    { label: "Add to queue", action: () => actions.addToQueue(track) },
    { label: "Play next", action: () => actions.playNext(track) },
    { label: "", separator: true },
    { label: "Copy link", action: () => actions.copyLink(track.url) },
    {
      label: `Open on ${platformLabel(track.platform)}`,
      action: () => actions.openOnPlatform(track.url),
    },
  ];
}
