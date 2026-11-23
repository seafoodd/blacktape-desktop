import type { Song } from "@/shared/types.ts";

export function shouldShowTrackArtist(song: Pick<Song, "album_artist" | "artists">): boolean {
  if (song.album_artist === "Various Artists") {
    return (song.artists && song.artists.length > 0) || false;
  }
  if (!song.artists || song.artists.length === 0) {
    return false;
  }
  return song.artists.length > 1 || song.artists[0] !== song.album_artist;
}

export function formatTrackArtists(song: Pick<Song, "album_artist" | "artists">): string {
  if (song.artists && song.artists.length > 0) {
    return song.artists.join(", ");
  }
  return song.album_artist !== "Various Artists" ? song.album_artist : "";
}
