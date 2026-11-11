import { describe, expect, it } from "vitest";
import { albumTarget, remoteTrackRef, streamSongToRemoteTrack } from "./currentSong.ts";
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";

describe("albumTarget", () => {
  it("opens the artist page for a library song with an album and an album artist", () => {
    expect(albumTarget({ album: "LP", album_artist: "Band" }, false, undefined)).toEqual({
      kind: "library",
      albumArtist: "Band",
    });
  });

  it("ignores the album url for a library song", () => {
    expect(albumTarget({ album: "LP", album_artist: "Band" }, false, "https://alb")).toEqual({
      kind: "library",
      albumArtist: "Band",
    });
  });

  it("returns null for a library song with an empty or whitespace album", () => {
    expect(albumTarget({ album: "", album_artist: "Band" }, false, undefined)).toBeNull();
    expect(albumTarget({ album: "   ", album_artist: "Band" }, false, undefined)).toBeNull();
  });

  it("returns null for a library song with an empty album artist", () => {
    expect(albumTarget({ album: "LP", album_artist: "" }, false, undefined)).toBeNull();
    expect(albumTarget({ album: "LP", album_artist: "  " }, false, undefined)).toBeNull();
  });

  it("opens the album preview for a stream with an album url", () => {
    expect(albumTarget({ album: "LP", album_artist: "" }, true, "https://alb")).toEqual({
      kind: "preview",
      url: "https://alb",
    });
  });

  it("returns null for a stream without an album url", () => {
    expect(albumTarget({ album: "LP", album_artist: "Band" }, true, undefined)).toBeNull();
  });

  it("returns null for a stream with an empty album even when it has an album url", () => {
    expect(albumTarget({ album: "", album_artist: "Band" }, true, "https://alb")).toBeNull();
  });
});

describe("streamSongToRemoteTrack", () => {
  const song = {
    title: "Song",
    artists: ["Feat Artist", "Other"],
    album_artist: "Album Artist",
    album: "LP",
    cover_url: "https://img/c.jpg",
    duration_ms: 185000,
    source: "bandcamp" as const,
    source_url: "https://b.bandcamp.com/track/song",
  };

  it("maps the streamed song to a remote track", () => {
    expect(streamSongToRemoteTrack(song, "https://b.bandcamp.com/album/lp")).toEqual({
      platform: "bandcamp",
      url: "https://b.bandcamp.com/track/song",
      title: "Song",
      artist: "Feat Artist",
      album: "LP",
      album_url: "https://b.bandcamp.com/album/lp",
      cover: "https://img/c.jpg",
      duration_ms: 185000,
    });
  });

  it("uses the album artist when the song has no artists", () => {
    const t = streamSongToRemoteTrack({ ...song, artists: [] }, undefined);
    expect(t?.artist).toBe("Album Artist");
  });

  it("uses the album artist when the first artist is empty", () => {
    const t = streamSongToRemoteTrack({ ...song, artists: [""] }, undefined);
    expect(t?.artist).toBe("Album Artist");
  });

  it("maps an empty album to a null album", () => {
    expect(streamSongToRemoteTrack({ ...song, album: "" }, undefined)?.album).toBeNull();
  });

  it("maps a missing album url to a null album_url", () => {
    expect(streamSongToRemoteTrack(song, undefined)?.album_url).toBeNull();
  });

  it("keeps a missing cover as undefined", () => {
    expect(streamSongToRemoteTrack({ ...song, cover_url: undefined }, undefined)?.cover).toBeUndefined();
  });

  it("maps a zero duration to a null duration_ms", () => {
    expect(streamSongToRemoteTrack({ ...song, duration_ms: 0 }, undefined)?.duration_ms).toBeNull();
  });

  it("keeps a positive duration", () => {
    expect(streamSongToRemoteTrack({ ...song, duration_ms: 1 }, undefined)?.duration_ms).toBe(1);
  });

  it("returns null when the source url is null", () => {
    expect(streamSongToRemoteTrack({ ...song, source_url: null }, undefined)).toBeNull();
  });

  it("returns null when the source url is undefined", () => {
    expect(streamSongToRemoteTrack({ ...song, source_url: undefined }, undefined)).toBeNull();
  });

  it("returns null when the source url is empty", () => {
    expect(streamSongToRemoteTrack({ ...song, source_url: "" }, undefined)).toBeNull();
  });
});

describe("remoteTrackRef", () => {
  it("returns the platform, url, artist and title of the track", () => {
    const track: RemoteTrack = {
      platform: "youtube",
      url: "https://yt.test/watch?v=1",
      title: "Song",
      artist: "Band",
      album: "LP",
      album_url: "https://alb",
      cover: "https://img/c.jpg",
      duration_ms: 1000,
    };
    expect(remoteTrackRef(track)).toEqual({
      platform: "youtube",
      url: "https://yt.test/watch?v=1",
      artist: "Band",
      title: "Song",
      album: "LP",
    });
  });

  it("gives the album as undefined when the track has a null album", () => {
    const track: RemoteTrack = {
      platform: "bandcamp",
      url: "https://b.test/track/1",
      title: "Song",
      artist: "Band",
      album: null,
      album_url: null,
      cover: undefined,
      duration_ms: null,
    };
    expect(remoteTrackRef(track).album).toBeUndefined();
  });

  it("gives the album as undefined when the track has an empty album", () => {
    const track: RemoteTrack = {
      platform: "bandcamp",
      url: "https://b.test/track/1",
      title: "Song",
      artist: "Band",
      album: "",
      album_url: null,
      cover: undefined,
      duration_ms: null,
    };
    expect(remoteTrackRef(track).album).toBeUndefined();
  });
});
