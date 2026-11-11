/**
 * A track that lives on a platform (not in the library): the common shape of search rows, album preview rows
 * and "missing" rows, used by the shared remote-track menu and the stream actions. Pure helpers.
 */
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { AlbumPreview, AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { StreamRequest } from "@/shared/lib/remote/stream.ts";
import type { Platform } from "@/shared/types.ts";

export interface RemoteTrack {
  platform: Platform;
  /** Page url of the track. */
  url: string;
  title: string;
  artist: string;
  album?: string | null;
  /** Page url of the album the track belongs to. When set, "Download" uses the album pipeline (same folder). */
  album_url?: string | null;
  cover?: string | null;
  duration_ms?: number | null;
}

export function searchItemToRemoteTrack(item: SearchSuggestion): RemoteTrack {
  return {
    platform: item.platform,
    url: item.item_url_path,
    title: item.name,
    artist: item.band_name,
    album: item.album_name ?? null,
    album_url: null,
    cover: item.img || null,
    duration_ms: typeof item.duration === "number" ? item.duration * 1000 : null,
  };
}

export function albumTrackToRemoteTrack(
  preview: Pick<AlbumPreview, "platform" | "url" | "title" | "artist" | "cover_url">,
  track: AlbumTrackInfo,
): RemoteTrack {
  return {
    platform: preview.platform,
    url: track.url,
    title: track.title,
    artist: track.artists[0] || preview.artist,
    album: preview.title,
    album_url: preview.url,
    cover: preview.cover_url,
    duration_ms: track.duration_ms ?? null,
  };
}

export function remoteTrackToStreamRequest(track: RemoteTrack): StreamRequest {
  return {
    platform: track.platform,
    url: track.url,
    title: track.title,
    artist: track.artist,
    album: track.album ?? undefined,
    album_url: track.album_url ?? undefined,
    cover: track.cover || null,
    duration_ms: track.duration_ms ?? null,
  };
}

/** The track as a search item (what `useStreamStore.play` takes). */
export function remoteTrackToStreamItem(track: RemoteTrack): SearchSuggestion {
  return {
    item_type: ItemType.Track,
    name: track.title,
    band_name: track.artist,
    album_name: track.album ?? undefined,
    item_url_path: track.url,
    img: track.cover ?? "",
    duration: typeof track.duration_ms === "number" ? Math.round(track.duration_ms / 1000) : undefined,
    platform: track.platform,
  };
}
