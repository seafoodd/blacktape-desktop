import type { AlbumTrackInfo, AlbumPreview } from "@/shared/lib/albumPreview.ts";
import type { LibraryAlbumSource, MissingTracks } from "@/shared/lib/libraryIndex.ts";
import {
  albumTrackToRemoteTrack,
  remoteTrackToStreamItem,
  type RemoteTrack,
} from "@/shared/lib/remoteTrack.ts";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { MenuItemOption } from "@/shared/ui/context-menu";
import { playableMissingTracks } from "@/features/library/model/missingTracksStore.ts";

function albumOf(
  missing: MissingTracks,
): Pick<AlbumPreview, "platform" | "url" | "title" | "artist" | "cover_url"> {
  return {
    platform: missing.platform,
    url: missing.source_url,
    title: missing.album_title,
    artist: missing.album_artist,
    cover_url: missing.cover_url,
  };
}

/**
 * The missing songs as remote tracks of their album. `album_url` is the source url, so "Download" in the menu
 * uses the album pipeline and the song lands in the album folder.
 */
export function missingRemoteTracks(missing: MissingTracks): RemoteTrack[] {
  const album = albumOf(missing);
  return playableMissingTracks(missing).map((track) => albumTrackToRemoteTrack(album, track));
}

/** The remote track of one missing song. */
export function missingRemoteTrack(missing: MissingTracks, track: AlbumTrackInfo): RemoteTrack {
  return albumTrackToRemoteTrack(albumOf(missing), track);
}

/** The missing songs as search items (what the stream store plays as a queue). */
export function missingStreamItems(missing: MissingTracks): SearchSuggestion[] {
  return missingRemoteTracks(missing).map(remoteTrackToStreamItem);
}

/** Track number of a missing song, two digits. Unknown numbers fall back to the position in the album. */
export function missingTrackNumber(track: Pick<AlbumTrackInfo, "track_number" | "index">): string {
  return String(track.track_number ?? track.index + 1).padStart(2, "0");
}

export interface MissingMenuActions {
  toggle: () => void;
  download: () => void;
}

/**
 * Menu entries of a library album that has a source: "Show missing songs (N)" / "Hide missing songs" and
 * "Download missing (N)". Empty when the album has no source. The show entry is left out when there is nothing
 * missing and nothing shown, and the download entry when nothing is missing.
 */
export function missingAlbumMenuItems(
  source: LibraryAlbumSource | undefined,
  shown: boolean,
  actions: MissingMenuActions,
): MenuItemOption[] {
  if (!source) return [];
  const items: MenuItemOption[] = [{ separator: true, label: "" }];
  if (source.missing > 0 || shown) {
    items.push({
      label: shown ? "Hide missing songs" : `Show missing songs (${source.missing})`,
      action: actions.toggle,
    });
  }
  if (source.missing > 0) {
    items.push({
      label: `Download missing (${source.missing})`,
      action: actions.download,
    });
  }
  return items.length > 1 ? items : [];
}
