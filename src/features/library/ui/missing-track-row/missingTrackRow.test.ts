import { describe, it, expect, vi } from "vitest";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { LibraryAlbumSource, MissingTracks } from "@/shared/lib/libraryIndex.ts";
import type { Song } from "@/shared/types.ts";
import {
  mergeAlbumRows,
  candidateMeta,
  missingAlbumMenuItems,
  missingRemoteTrack,
  missingRemoteTracks,
  missingStreamItems,
  missingTrackNumber,
  missingTrackPercent,
  missingTrackDownloading,
  nextGhosts,
  ghostTrack,
  looseTitle,
  GHOST_TTL_MS,
  songMatchesTrack,
  withoutDownloaded,
  type Ghost,
} from "./missingTrackRow.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/libraryIndex.ts", () => ({
  getArtistAlbumSources: vi.fn(),
  getMissingTracks: vi.fn(),
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: { getState: () => ({ startDownload: vi.fn() }), subscribe: vi.fn() },
}));

const track = (index: number, over: Partial<AlbumTrackInfo> = {}): AlbumTrackInfo => ({
  index,
  title: `Track ${index + 1}`,
  artists: ["Guest"],
  track_number: index + 1,
  url: `https://example.test/track/${index}`,
  available: true,
  note: null,
  duration_ms: 185000,
  ...over,
});

const missing = (over: Partial<MissingTracks> = {}): MissingTracks => ({
  platform: "bandcamp",
  source_url: "https://example.test/album/A",
  album_title: "A",
  album_artist: "Artist",
  cover_url: "https://example.test/cover.jpg",
  tracks: [track(1)],
  ...over,
});

const source = (over: Partial<LibraryAlbumSource> = {}): LibraryAlbumSource => ({
  album: "A",
  platform: "bandcamp",
  source_url: "https://example.test/album/A",
  cover_url: null,
  total: 4,
  owned: 2,
  missing: 2,
  ...over,
});

const actions = () => ({ toggle: vi.fn(), download: vi.fn() });

describe("missingRemoteTracks", () => {
  it("maps the playable missing songs to remote tracks of the source album", () => {
    const list = missingRemoteTracks(
      missing({ tracks: [track(0), track(1, { available: false, url: "" }), track(2)] }),
    );
    expect(list.map((t) => t.url)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/2",
    ]);
    expect(list[0]).toEqual({
      platform: "bandcamp",
      url: "https://example.test/track/0",
      title: "Track 1",
      artist: "Guest",
      album: "A",
      album_url: "https://example.test/album/A",
      cover: "https://example.test/cover.jpg",
      duration_ms: 185000,
    });
  });

  it("falls back to the album artist when the track names no artist", () => {
    const [remote] = missingRemoteTracks(missing({ tracks: [track(0, { artists: [] })] }));
    expect(remote.artist).toBe("Artist");
  });

  it("is empty when nothing is missing", () => {
    expect(missingRemoteTracks(missing({ tracks: [] }))).toEqual([]);
  });
});

describe("missingRemoteTrack", () => {
  it("gives the remote track of one song, with a null duration when unknown", () => {
    const remote = missingRemoteTrack(missing(), track(1, { duration_ms: null }));
    expect(remote.url).toBe("https://example.test/track/1");
    expect(remote.album_url).toBe("https://example.test/album/A");
    expect(remote.duration_ms).toBeNull();
  });

  it("uses a null cover when the album has none", () => {
    const remote = missingRemoteTrack(missing({ cover_url: null }), track(1));
    expect(remote.cover).toBeNull();
  });
});

describe("missingStreamItems", () => {
  it("gives search items in the same order, with the duration in seconds", () => {
    const items = missingStreamItems(missing({ tracks: [track(0), track(1)] }));
    expect(items.map((i) => i.item_url_path)).toEqual([
      "https://example.test/track/0",
      "https://example.test/track/1",
    ]);
    expect(items[0].duration).toBe(185);
    expect(items[0].platform).toBe("bandcamp");
    expect(items[0].band_name).toBe("Guest");
  });
});

describe("missingTrackNumber", () => {
  it("uses the track number, padded to two digits", () => {
    expect(missingTrackNumber({ track_number: 7, index: 0 })).toBe("07");
  });

  it("falls back to the position in the album when the number is unknown", () => {
    expect(missingTrackNumber({ track_number: null, index: 11 })).toBe("12");
  });
});

describe("missingAlbumMenuItems", () => {
  it("has no entries for an album without a source", () => {
    expect(missingAlbumMenuItems(undefined, false, actions())).toEqual([]);
  });

  it("offers show and download when songs are missing and none is shown", () => {
    const a = actions();
    const items = missingAlbumMenuItems(source({ missing: 2 }), false, a);
    expect(items.map((i) => i.label)).toEqual([
      "",
      "Show missing songs (2)",
      "Download missing (2)",
    ]);
    expect(items[0].separator).toBe(true);
    items[1].action?.();
    items[2].action?.();
    expect(a.toggle).toHaveBeenCalledTimes(1);
    expect(a.download).toHaveBeenCalledTimes(1);
  });

  it("offers hide, and download while songs are missing, when the missing songs are shown", () => {
    const items = missingAlbumMenuItems(source({ missing: 1 }), true, actions());
    expect(items.map((i) => i.label)).toEqual(["", "Hide missing songs", "Download missing (1)"]);
  });

  it("offers nothing when no song is missing", () => {
    expect(missingAlbumMenuItems(source({ missing: 0 }), false, actions())).toEqual([]);
  });

  it("still offers hide when the shown songs are all in the library now", () => {
    const items = missingAlbumMenuItems(source({ missing: 0 }), true, actions());
    expect(items.map((i) => i.label)).toEqual(["", "Hide missing songs"]);
  });
});

const song = (track_number: number | null | undefined, title = `Song ${track_number}`): Song =>
  ({
    id: 1,
    path: "/music/song.mp3",
    title,
    artists: ["Guest"],
    album_artist: "Artist",
    album: "A",
    duration_ms: 185000,
    track_number,
    source: "bandcamp",
  }) as Song;

const numberOf = (row: { kind: "song"; song: Song } | { kind: "missing"; track: AlbumTrackInfo }) =>
  row.kind === "song" ? row.song.track_number : row.track.track_number;

describe("mergeAlbumRows", () => {
  it("interleaves songs and missing songs by track number", () => {
    const rows = mergeAlbumRows(
      [song(7), song(8)],
      [1, 2, 3, 4, 5, 6, 9].map((n) => track(n - 1)),
    );
    expect(rows.map(numberOf)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(rows.map((r) => r.kind)).toEqual([
      "missing",
      "missing",
      "missing",
      "missing",
      "missing",
      "missing",
      "song",
      "song",
      "missing",
    ]);
  });

  it("keeps a song without a track number right after the song before it", () => {
    const rows = mergeAlbumRows(
      [song(3, "a"), song(null, "b")],
      [track(0), track(3)],
    );
    expect(rows.map((r) => (r.kind === "song" ? r.song.title : `missing ${numberOf(r)}`))).toEqual([
      "missing 1",
      "a",
      "b",
      "missing 4",
    ]);
  });

  it("puts songs first when a song and a missing song have the same number", () => {
    const rows = mergeAlbumRows([song(2, "a")], [track(1)]);
    expect(rows.map((r) => r.kind)).toEqual(["song", "missing"]);
  });

  it("returns the songs unchanged when nothing is missing", () => {
    const songs = [song(1), song(2), song(3)];
    expect(mergeAlbumRows(songs, [])).toEqual(songs.map((s) => ({ kind: "song", song: s })));
  });
});

const dl = (tracks: { title: string; percent: number; status: "pending" | "downloading" | "completed" | "failed"; trackNumber?: number }[]) =>
  ({ tracks });

describe("missingTrackPercent", () => {
  it("is 0 when there is no running download", () => {
    expect(missingTrackPercent(undefined, { title: "Track 1", track_number: 1, index: 0 })).toBe(0);
  });

  it("matches the track by title, ignoring case", () => {
    const download = dl([{ title: "HELLO world", percent: 40, status: "downloading" }]);
    expect(missingTrackPercent(download, { title: "hello WORLD", track_number: 5, index: 4 })).toBe(40);
  });

  it("falls back to the track number when the title does not match", () => {
    const download = dl([{ title: "Other", percent: 25, status: "downloading", trackNumber: 3 }]);
    expect(missingTrackPercent(download, { title: "Different", track_number: 3, index: 2 })).toBe(25);
  });

  it("is 100 when the track has completed", () => {
    const download = dl([{ title: "Track 1", percent: 10, status: "completed" }]);
    expect(missingTrackPercent(download, { title: "Track 1", track_number: 1, index: 0 })).toBe(100);
  });

  it("clamps the percent to 0..100", () => {
    const track1 = { title: "Track 1", track_number: 1, index: 0 };
    expect(missingTrackPercent(dl([{ title: "Track 1", percent: 150, status: "downloading" }]), track1)).toBe(100);
    expect(missingTrackPercent(dl([{ title: "Track 1", percent: -20, status: "downloading" }]), track1)).toBe(0);
  });

  it("rounds the percent to a whole number", () => {
    const track1 = { title: "Track 1", track_number: 1, index: 0 };
    expect(missingTrackPercent(dl([{ title: "Track 1", percent: 33.6, status: "downloading" }]), track1)).toBe(34);
    expect(missingTrackPercent(dl([{ title: "Track 1", percent: 33.4, status: "downloading" }]), track1)).toBe(33);
  });

  it("is 0 for a track the download does not list", () => {
    const download = dl([{ title: "Other", percent: 50, status: "downloading", trackNumber: 9 }]);
    expect(missingTrackPercent(download, { title: "Track 1", track_number: 1, index: 0 })).toBe(0);
  });
});

/** A library song with a given id, title and source url (other fields from the `song` factory). */
const songAt = (id: number, title: string, source_url: string | null = null): Song => ({
  ...song(1, title),
  id,
  source_url,
});

describe("songMatchesTrack", () => {
  it("matches by the same page url, ignoring host prefix, query, fragment and trailing slash", () => {
    expect(
      songMatchesTrack(
        songAt(1, "Library name", "https://www.youtube.com/watch?v=AbC123&t=5"),
        track(0, { title: "Other", url: "https://youtu.be/AbC123" }),
      ),
    ).toBe(true);
    expect(
      songMatchesTrack(
        songAt(1, "Library name", "https://m.example.test/Track/One/?a=1#x"),
        track(0, { title: "Other", url: "https://example.test/track/one" }),
      ),
    ).toBe(true);
  });

  it("does not match when neither the url nor the title is the same", () => {
    expect(
      songMatchesTrack(
        songAt(1, "One", "https://example.test/track/one"),
        track(0, { title: "Two", url: "https://example.test/track/two" }),
      ),
    ).toBe(false);
  });

  it("matches by title, ignoring case and surrounding spaces", () => {
    expect(
      songMatchesTrack(songAt(1, "  HELLO world "), track(0, { title: "hello World", url: "https://a.test/1" })),
    ).toBe(true);
  });

  it("never matches a ghost track (empty url) by url", () => {
    const ghost = ghostTrack(songAt(1, "Gone", "https://example.test/track/0"), 0);
    expect(ghost.url).toBe("");
    expect(songMatchesTrack(songAt(2, "Other", "https://example.test/track/0"), ghost)).toBe(false);
    expect(songMatchesTrack(songAt(3, "Other", ""), ghost)).toBe(false);
  });

  it("still matches a ghost track by title", () => {
    const ghost = ghostTrack(songAt(1, "Gone"), 0);
    expect(songMatchesTrack(songAt(2, "gone"), ghost)).toBe(true);
  });
});

describe("withoutDownloaded", () => {
  it("drops the missing tracks that are already library songs and keeps the others in order", () => {
    const missingTracks = [
      track(0, { title: "First", url: "https://example.test/track/0" }),
      track(1, { title: "Second", url: "https://example.test/track/1" }),
      track(2, { title: "Third", url: "https://example.test/track/2" }),
    ];
    const songs = [
      songAt(1, "Something else", "https://www.example.test/track/1/?x=1"),
      songAt(2, "third"),
    ];
    expect(withoutDownloaded(missingTracks, songs).map((t) => t.title)).toEqual(["First"]);
  });

  it("keeps every missing track when no song matches", () => {
    const missingTracks = [track(0), track(1)];
    expect(withoutDownloaded(missingTracks, [songAt(1, "Unrelated")])).toEqual(missingTracks);
  });

  it("keeps every missing track when there are no songs, and is empty for no missing tracks", () => {
    const missingTracks = [track(0)];
    expect(withoutDownloaded(missingTracks, [])).toEqual(missingTracks);
    expect(withoutDownloaded([], [songAt(1, "Track 1")])).toEqual([]);
  });
});

describe("nextGhosts", () => {
  const now = 10_000;

  it("turns a song removed from prevSongs into a ghost stamped with now", () => {
    const gone = songAt(1, "Gone");
    const next = nextGhosts([gone, songAt(2, "Kept")], [songAt(2, "Kept")], [], new Map(), now);
    expect(next.size).toBe(1);
    expect(next.get(1)).toEqual({ song: gone, at: now });
  });

  it("does not ghost a removed song that the missing list already contains by title", () => {
    const next = nextGhosts(
      [songAt(1, "Gone")],
      [],
      [track(0, { title: "gone", url: "https://example.test/track/0" })],
      new Map(),
      now,
    );
    expect(next.size).toBe(0);
  });

  it("drops a ghost once the missing list contains its title", () => {
    const ghosts = new Map<number, Ghost>([[1, { song: songAt(1, "Gone"), at: now }]]);
    const next = nextGhosts(undefined, [], [track(0, { title: "Gone", url: "" })], ghosts, now);
    expect(next.size).toBe(0);
  });

  it("drops a ghost when its song comes back to the library", () => {
    const ghosts = new Map<number, Ghost>([[1, { song: songAt(1, "Gone"), at: now }]]);
    const next = nextGhosts(undefined, [songAt(1, "Gone")], [], ghosts, now);
    expect(next.size).toBe(0);
  });

  it("drops a ghost once it is GHOST_TTL_MS old, and keeps it just before that", () => {
    const ghosts = new Map<number, Ghost>([[1, { song: songAt(1, "Gone"), at: now }]]);
    expect(nextGhosts(undefined, [], [], ghosts, now + GHOST_TTL_MS - 1).has(1)).toBe(true);
    expect(nextGhosts(undefined, [], [], ghosts, now + GHOST_TTL_MS).has(1)).toBe(false);
  });

  it("creates no ghosts when prevSongs is undefined", () => {
    expect(nextGhosts(undefined, [songAt(1, "A")], [], new Map(), now).size).toBe(0);
  });

  it("keeps an existing ghost with its original stamp, even when prevSongs still has the song", () => {
    const gone = songAt(1, "Gone");
    const ghosts = new Map<number, Ghost>([[1, { song: gone, at: now - 1000 }]]);
    expect(nextGhosts(undefined, [], [], ghosts, now).get(1)).toEqual({ song: gone, at: now - 1000 });
    expect(nextGhosts([gone], [], [], ghosts, now).get(1)).toEqual({ song: gone, at: now - 1000 });
  });
});

describe("ghostTrack", () => {
  it("has an empty url and carries the title, duration and track number of the song", () => {
    const ghost = ghostTrack({ ...songAt(1, "Gone"), track_number: 4, duration_ms: 123000 }, 3);
    expect(ghost).toEqual({
      index: 3,
      title: "Gone",
      artists: [],
      track_number: 4,
      url: "",
      available: true,
      note: null,
      duration_ms: 123000,
    });
  });

  it("has a null track number when the song has none", () => {
    expect(ghostTrack(song(undefined, "No number"), 0).track_number).toBeNull();
  });
});

describe("GHOST_TTL_MS", () => {
  it("is 3 seconds", () => {
    expect(GHOST_TTL_MS).toBe(3000);
  });
});

describe("candidateMeta", () => {
  it("names the known platforms", () => {
    expect(candidateMeta({ platform: "youtube", year: null })).toBe("YouTube");
    expect(candidateMeta({ platform: "bandcamp", year: null })).toBe("Bandcamp");
    expect(candidateMeta({ platform: "soundcloud", year: null })).toBe("SoundCloud");
  });

  it("matches the platform ignoring case", () => {
    expect(candidateMeta({ platform: "Bandcamp", year: null })).toBe("Bandcamp");
    expect(candidateMeta({ platform: "SOUNDCLOUD", year: null })).toBe("SoundCloud");
  });

  it("adds the year after a middle dot", () => {
    expect(candidateMeta({ platform: "bandcamp", year: 2019 })).toBe("Bandcamp · 2019");
  });

  it("gives the platform only when the year is null", () => {
    expect(candidateMeta({ platform: "youtube", year: null })).not.toContain("·");
  });

  it("passes an unknown platform through unchanged", () => {
    expect(candidateMeta({ platform: "MySpace", year: null })).toBe("MySpace");
    expect(candidateMeta({ platform: "MySpace", year: 2005 })).toBe("MySpace · 2005");
  });
});

describe("missingAlbumMenuItems without a source and with find", () => {
  it("offers a separator and Find missing songs, which calls find", () => {
    const a = { ...actions(), find: vi.fn() };
    const items = missingAlbumMenuItems(undefined, false, a);
    expect(items.map((i) => i.label)).toEqual(["", "Find missing songs..."]);
    expect(items[0].separator).toBe(true);
    items[1].action?.();
    expect(a.find).toHaveBeenCalledTimes(1);
    expect(a.toggle).not.toHaveBeenCalled();
    expect(a.download).not.toHaveBeenCalled();
  });

  it("is empty without a source and without find", () => {
    expect(missingAlbumMenuItems(undefined, false, actions())).toEqual([]);
  });
});

describe("looseTitle", () => {
  it("drops text in parentheses and brackets", () => {
    expect(looseTitle("Title(+ Guest)")).toBe("title");
    expect(looseTitle("[Remastered] Song!")).toBe("song");
  });

  it("ignores case and punctuation", () => {
    expect(looseTitle("Hello, World!!")).toBe("hello world");
    expect(looseTitle("  HELLO   world ")).toBe("hello world");
  });

  it("keeps Cyrillic letters", () => {
    expect(looseTitle("Бля, Сосны!(+ Гость)")).toBe("бля сосны");
  });
});

describe("songMatchesTrack with a guest suffix in the library title", () => {
  it("matches the plain remote title", () => {
    const song = songAt(1, "Title(+ Guest)", "https://library.test/other");
    expect(songMatchesTrack(song, track(0, { title: "Title", url: "https://remote.test/title" }))).toBe(true);
  });
});

describe("missingAlbumMenuItems with find and unlink", () => {
  it("offers Change linked album and Unlink album after the download entry, which call find and unlink", () => {
    const a = { ...actions(), find: vi.fn(), unlink: vi.fn() };
    const items = missingAlbumMenuItems(source({ missing: 2 }), false, a);
    expect(items.map((i) => i.label)).toEqual([
      "",
      "Show missing songs (2)",
      "Download missing (2)",
      "Change linked album...",
      "Unlink album",
    ]);
    items[3].action?.();
    items[4].action?.();
    expect(a.find).toHaveBeenCalledTimes(1);
    expect(a.unlink).toHaveBeenCalledTimes(1);
    expect(a.toggle).not.toHaveBeenCalled();
    expect(a.download).not.toHaveBeenCalled();
  });

  it("leaves Change linked album and Unlink album out when find and unlink are not given", () => {
    const labels = missingAlbumMenuItems(source({ missing: 2 }), false, actions()).map((i) => i.label);
    expect(labels).not.toContain("Change linked album...");
    expect(labels).not.toContain("Unlink album");
    expect(labels).toEqual(["", "Show missing songs (2)", "Download missing (2)"]);
  });
});

describe("songMatchesTrack brackets", () => {
  const song = (title: string) => ({ title, source_url: null }) as Parameters<typeof songMatchesTrack>[0];
  const track = (title: string) => ({ title, url: "https://u/x" }) as Parameters<typeof songMatchesTrack>[1];

  it("does not treat a bracketed remote track as the plain song", () => {
    expect(songMatchesTrack(song("Взаперти"), track("Взаперти (интерлюдия)"))).toBe(false);
  });

  it("still matches a song with a guest suffix to the plain remote title", () => {
    expect(songMatchesTrack(song("Title(+ Guest)"), track("Title"))).toBe(true);
  });
});

describe("missingTrackDownloading", () => {
  const track = { title: "Kid War", track_number: 11, index: 10 };
  const dl = (status: string, titles: string[]) =>
    ({ status, tracks: titles.map((title) => ({ title, trackNumber: 1, percent: 0, status: "downloading" })) }) as never;

  it("is false without a download or when it is finished", () => {
    expect(missingTrackDownloading(undefined, track)).toBe(false);
    expect(missingTrackDownloading(dl("completed", ["Kid War"]), track)).toBe(false);
  });

  it("is true for a listed track or before the track list arrives", () => {
    expect(missingTrackDownloading(dl("downloading", ["kid war"]), track)).toBe(true);
    expect(missingTrackDownloading(dl("idle", []), track)).toBe(true);
  });

  it("is false for a track the download does not list", () => {
    expect(missingTrackDownloading(dl("downloading", ["Other"]), { ...track, track_number: 99 })).toBe(false);
  });
});
