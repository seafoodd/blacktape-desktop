import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";
import type { FullSearchPage, SearchKind } from "@/shared/lib/search/searchFull.ts";
import { useSearchPageStore } from "./searchPageStore.ts";

const { searchFullMock, getArtistProfileMock, listenMock, listeners } = vi.hoisted(() => {
  const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
  const listenMock = vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    const handlers = listeners.get(event) ?? new Set();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => handlers.delete(handler);
  });
  return { searchFullMock: vi.fn(), getArtistProfileMock: vi.fn(), listenMock, listeners };
});

vi.mock("@/shared/lib/search/searchFull.ts", () => ({
  searchFull: searchFullMock,
  getArtistProfile: getArtistProfileMock,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({ getArtistAlbums: vi.fn(), getArtists: vi.fn() }));
vi.mock("@/shared/lib/search/search.ts", () => ({ searchPlatforms: vi.fn() }));

const emptyPage = (warnings: FullSearchPage["warnings"]): FullSearchPage => ({
  items: [],
  copies: [],
  next_cursor: null,
  has_more: false,
  warnings,
});

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  searchFullMock.mockReset();
  getArtistProfileMock.mockReset();
  listeners.clear();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  useSearchPageStore.setState({ ...useSearchPageStore.getState(), query: "" });
  useLibraryStore.setState({ activeView: "ARTIST_ALBUMS" });
});

afterEach(() => vi.unstubAllGlobals());

describe("full search warning attribution", () => {
  it("attributes timeout and rate-limit messages to SoundCloud, Bandcamp, and MusicBrainz", async () => {
    searchFullMock.mockImplementation(({ kind }: { kind: SearchKind }) => {
      if (kind === ItemType.Track) {
        return Promise.resolve(emptyPage([{
          platform: "soundcloud",
          source: "soundcloud",
          message: "request timed out",
        }]));
      }
      if (kind === ItemType.Album) {
        return Promise.resolve(emptyPage([{
          platform: "bandcamp",
          source: "bandcamp",
          message: "429 Too Many Requests",
        }]));
      }
      return Promise.resolve(emptyPage([{
        platform: "local",
        source: "musicbrainz",
        message: "request timed out",
      }]));
    });

    useSearchPageStore.getState().open("artist");
    await vi.waitFor(() => expect(searchFullMock).toHaveBeenCalledTimes(3));
    await flush();

    const trackWarning = useSearchPageStore.getState().sections[ItemType.Track].warnings[0];
    const albumWarning = useSearchPageStore.getState().sections[ItemType.Album].warnings[0];
    const artistWarning = useSearchPageStore.getState().sections[ItemType.Artist].warnings[0];
    expect(trackWarning).toContain("SoundCloud");
    expect(albumWarning).toContain("Bandcamp");
    expect(artistWarning).toContain("MusicBrainz");
    for (const warning of [trackWarning, albumWarning, artistWarning]) {
      expect(warning).not.toContain("YouTube");
    }
  });
});
