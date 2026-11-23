import { beforeEach, describe, expect, it, vi } from "vitest";
import { useFavoritesStore } from "./favoritesStore.ts";
import * as audioApi from "@/shared/lib/audio.ts";
import { Song } from "@/shared/types.ts";

vi.mock("@/shared/lib/audio.ts", () => ({
  getFavoriteIds: vi.fn(),
  getFavoriteSongs: vi.fn(),
  toggleFavorite: vi.fn(),
  addFavorites: vi.fn(),
  removeFavorites: vi.fn(),
}));

const mockSong: Song = {
  id: 1,
  path: "/music/song1.mp3",
  title: "Song 1",
  artists: ["Artist A"],
  album_artist: "Artist A",
  album: "Album 1",
  duration_ms: 180000,
  source: "local",
  canonical_track_slug: "song-1",
  canonical_album_slug: "album-1",
  quality_tier: "standard",
};

const mockSong2: Song = {
  id: 2,
  path: "/music/song2.mp3",
  title: "Song 2",
  artists: ["Artist B"],
  album_artist: "Artist B",
  album: "Album 2",
  duration_ms: 200000,
  source: "local",
  canonical_track_slug: "song-2",
  canonical_album_slug: "album-2",
  quality_tier: "standard",
};

describe("favoritesStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useFavoritesStore.setState({
      favoriteIds: new Set(),
      favoriteSongs: [],
      loading: false,
    });
  });

  it("fetches favorite ids and updates state", async () => {
    vi.mocked(audioApi.getFavoriteIds).mockResolvedValue([1, 2]);

    await useFavoritesStore.getState().fetchFavoriteIds();

    expect(audioApi.getFavoriteIds).toHaveBeenCalledTimes(1);
    expect(useFavoritesStore.getState().favoriteIds).toEqual(new Set([1, 2]));
    expect(useFavoritesStore.getState().isFavorite(1)).toBe(true);
    expect(useFavoritesStore.getState().isFavorite(3)).toBe(false);
  });

  it("fetches favorite songs and syncs favorite ids", async () => {
    vi.mocked(audioApi.getFavoriteSongs).mockResolvedValue([mockSong, mockSong2]);

    await useFavoritesStore.getState().fetchFavoriteSongs();

    expect(audioApi.getFavoriteSongs).toHaveBeenCalledTimes(1);
    expect(useFavoritesStore.getState().favoriteSongs).toEqual([mockSong, mockSong2]);
    expect(useFavoritesStore.getState().favoriteIds).toEqual(new Set([1, 2]));
  });

  it("toggles favorite on with optimistic update", async () => {
    vi.mocked(audioApi.toggleFavorite).mockResolvedValue(true);
    vi.mocked(audioApi.getFavoriteSongs).mockResolvedValue([mockSong]);

    const result = await useFavoritesStore.getState().toggleFavorite(1);

    expect(result).toBe(true);
    expect(audioApi.toggleFavorite).toHaveBeenCalledWith(1);
    expect(useFavoritesStore.getState().isFavorite(1)).toBe(true);
  });

  it("toggles favorite off with optimistic update", async () => {
    useFavoritesStore.setState({
      favoriteIds: new Set([1, 2]),
      favoriteSongs: [mockSong, mockSong2],
    });

    vi.mocked(audioApi.toggleFavorite).mockResolvedValue(false);

    const result = await useFavoritesStore.getState().toggleFavorite(1);

    expect(result).toBe(false);
    expect(audioApi.toggleFavorite).toHaveBeenCalledWith(1);
    expect(useFavoritesStore.getState().isFavorite(1)).toBe(false);
    expect(useFavoritesStore.getState().isFavorite(2)).toBe(true);
    expect(useFavoritesStore.getState().favoriteSongs).toEqual([mockSong2]);
  });

  it("reverts optimistic update on toggle error", async () => {
    useFavoritesStore.setState({
      favoriteIds: new Set([1]),
      favoriteSongs: [mockSong],
    });

    vi.mocked(audioApi.toggleFavorite).mockRejectedValue(new Error("DB error"));

    await expect(useFavoritesStore.getState().toggleFavorite(1)).rejects.toThrow("DB error");

    expect(useFavoritesStore.getState().isFavorite(1)).toBe(true);
    expect(useFavoritesStore.getState().favoriteSongs).toEqual([mockSong]);
  });

  it("adds multiple favorites and refreshes state", async () => {
    vi.mocked(audioApi.addFavorites).mockResolvedValue(undefined);
    vi.mocked(audioApi.getFavoriteIds).mockResolvedValue([1, 2]);
    vi.mocked(audioApi.getFavoriteSongs).mockResolvedValue([mockSong, mockSong2]);

    await useFavoritesStore.getState().addFavorites([1, 2]);

    expect(audioApi.addFavorites).toHaveBeenCalledWith([1, 2]);
    expect(audioApi.getFavoriteIds).toHaveBeenCalled();
    expect(audioApi.getFavoriteSongs).toHaveBeenCalled();
  });

  it("removes multiple favorites and refreshes state", async () => {
    vi.mocked(audioApi.removeFavorites).mockResolvedValue(undefined);
    vi.mocked(audioApi.getFavoriteIds).mockResolvedValue([]);
    vi.mocked(audioApi.getFavoriteSongs).mockResolvedValue([]);

    await useFavoritesStore.getState().removeFavorites([1, 2]);

    expect(audioApi.removeFavorites).toHaveBeenCalledWith([1, 2]);
    expect(audioApi.getFavoriteIds).toHaveBeenCalled();
    expect(audioApi.getFavoriteSongs).toHaveBeenCalled();
  });

  it("evaluates favorite selector safely for null, undefined, or song", () => {
    useFavoritesStore.setState({ favoriteIds: new Set([1]) });
    const isFavorite = (currentSong: { id: number } | null | undefined) =>
      currentSong ? useFavoritesStore.getState().favoriteIds.has(currentSong.id) : false;

    expect(isFavorite(null)).toBe(false);
    expect(isFavorite(undefined)).toBe(false);
    expect(isFavorite({ id: 1 })).toBe(true);
    expect(isFavorite({ id: 2 })).toBe(false);
  });
});
