import { describe, expect, it } from "vitest";
import { shouldShowTrackArtist, formatTrackArtists } from "./songListItemHelper.ts";

describe("SongListItem helper", () => {
  it("shows artist for Various Artists albums when track artists are present", () => {
    const song = {
      album_artist: "Various Artists",
      artists: ["Eminem"],
    };
    expect(shouldShowTrackArtist(song)).toBe(true);
    expect(formatTrackArtists(song)).toBe("Eminem");
  });

  it("formats multiple artists with comma separation", () => {
    const song = {
      album_artist: "Various Artists",
      artists: ["Obie Trice", "50 Cent", "Eminem"],
    };
    expect(shouldShowTrackArtist(song)).toBe(true);
    expect(formatTrackArtists(song)).toBe("Obie Trice, 50 Cent, Eminem");
  });

  it("does not show artist suffix when album artist matches the track artist", () => {
    const song = {
      album_artist: "Black Sabbath",
      artists: ["Black Sabbath"],
    };
    expect(shouldShowTrackArtist(song)).toBe(false);
  });

  it("shows artist suffix when track has featured artists differing from album artist", () => {
    const song = {
      album_artist: "Eminem",
      artists: ["Eminem", "Dr. Dre"],
    };
    expect(shouldShowTrackArtist(song)).toBe(true);
    expect(formatTrackArtists(song)).toBe("Eminem, Dr. Dre");
  });

  it("does not show artist suffix when artists array is empty on regular album", () => {
    const song = {
      album_artist: "Aphex Twin",
      artists: [],
    };
    expect(shouldShowTrackArtist(song)).toBe(false);
  });
});
