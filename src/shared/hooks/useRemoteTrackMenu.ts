import type { MouseEvent } from "react";
import {
  useRemoteTrackMenuStore,
  type RemoteTrackMenuOptions,
} from "@/features/remote-track-menu/model/remoteTrackMenuStore.ts";
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";

/**
 * The ONE way to open the context menu of a remote track (Album Preview rows, search-result track rows, ghost
 * rows). Use it as `onContextMenu={(e) => openMenu(e, track, { list })}`. The menu itself (Download, Add to
 * queue, Play next, Copy link, Open on platform) is rendered by `RemoteTrackMenuHost` (part F).
 */
export function useRemoteTrackMenu() {
  const openMenu = (e: MouseEvent, track: RemoteTrack, options?: RemoteTrackMenuOptions) => {
    e.preventDefault();
    e.stopPropagation();
    useRemoteTrackMenuStore.getState().openMenu({ x: e.clientX, y: e.clientY }, track, options);
  };
  return { openMenu };
}
