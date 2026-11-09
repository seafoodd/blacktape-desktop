import { describe, expect, it } from "vitest";
import { ItemType, type SearchSuggestion } from "@/shared/store/libraryStore.ts";
import {
  albumTrackToRemoteTrack,
  remoteTrackToStreamItem,
  remoteTrackToStreamRequest,
  searchItemToRemoteTrack,
} from "./remoteTrack.ts";

const item: SearchSuggestion = {
  item_type: ItemType.Track,
  name: "Song",
  band_name: "Band",
  album_name: "LP",
  item_url_path: "https://b.bandcamp.com/track/song",
  img: "https://img/c.jpg",
  duration: 185,
  platform: "bandcamp",
};

describe("remoteTrack converters", () => {
  it("maps a search item (seconds to ms, no album url)", () => {
    const t = searchItemToRemoteTrack(item);
    expect(t).toMatchObject({ url: item.item_url_path, title: "Song", artist: "Band", album: "LP", duration_ms: 185000 });
    expect(t.album_url).toBeNull();
  });

  it("maps an album preview row and keeps the album url", () => {
    const t = albumTrackToRemoteTrack(
      { platform: "bandcamp", url: "https://b.bandcamp.com/album/lp", title: "LP", artist: "Band", cover_url: null },
      { index: 0, title: "One", artists: [], track_number: 1, url: "https://b/1", available: true, note: null, duration_ms: 1000 },
    );
    expect(t.artist).toBe("Band");
    expect(t.album_url).toBe("https://b.bandcamp.com/album/lp");
    expect(t.duration_ms).toBe(1000);
  });

  it("round-trips to a stream item and request", () => {
    const t = searchItemToRemoteTrack(item);
    expect(remoteTrackToStreamItem(t)).toMatchObject({ item_url_path: item.item_url_path, duration: 185, name: "Song" });
    expect(remoteTrackToStreamRequest(t)).toEqual({
      platform: "bandcamp",
      url: item.item_url_path,
      title: "Song",
      artist: "Band",
      album: "LP",
      cover: "https://img/c.jpg",
      duration_ms: 185000,
    });
  });
});
