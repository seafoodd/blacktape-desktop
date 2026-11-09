/**
 * What the player's "now playing" block can do with the current song: open its album and download it when it is
 * only a stream. Pure helpers; the component wires them to the stores.
 */
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";
import type { RemoteRef } from "@/shared/lib/libraryIndex.ts";
import type { Song } from "@/shared/types.ts";

/** Where the album of the current song opens: the library's artist page, or the preview of the album page. */
export type AlbumTarget =
  | { kind: "library"; albumArtist: string }
  | { kind: "preview"; url: string };

/**
 * A library song opens its artist's albums; a stream opens the album preview when the album page is known (it
 * is not for every stream, e.g. a single track result). No album, no target.
 */
export function albumTarget(
  song: Pick<Song, "album" | "album_artist">,
  isStreaming: boolean,
  albumUrl: string | undefined,
): AlbumTarget | null {
  if (song.album.trim() === "") return null;
  if (!isStreaming) return song.album_artist.trim() === "" ? null : { kind: "library", albumArtist: song.album_artist };
  return albumUrl ? { kind: "preview", url: albumUrl } : null;
}

/** The streamed song as a remote track (what the download and the library-state check take). */
export function streamSongToRemoteTrack(
  song: Pick<Song, "title" | "artists" | "album_artist" | "album" | "cover_url" | "duration_ms" | "source" | "source_url">,
  albumUrl: string | undefined,
): RemoteTrack | null {
  if (!song.source_url) return null;
  return {
    platform: song.source,
    url: song.source_url,
    title: song.title,
    artist: song.artists[0] || song.album_artist,
    album: song.album || null,
    album_url: albumUrl ?? null,
    cover: song.cover_url,
    duration_ms: song.duration_ms > 0 ? song.duration_ms : null,
  };
}

/** The library-index ref of a remote track. */
export const remoteTrackRef = (track: RemoteTrack): RemoteRef => ({
  platform: track.platform,
  url: track.url,
  artist: track.artist,
  title: track.title,
});
