import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { playlistApi } from "@/shared/api/playlists";
import { getArtistAlbums } from "@/shared/lib/audio.ts";

/**
 * Reloads everything that shows song metadata after a library change:
 * artist list, the opened artist's albums and the opened playlist's songs.
 * (The player's current song and queue are pushed by the backend.)
 */
export async function refreshLibrary(): Promise<void> {
  const { selectedTab, fetchTabs } = useLibraryStore.getState();
  const { activePlaylistId } = usePlaylistStore.getState();

  const tasks: Promise<unknown>[] = [fetchTabs()];
  if (selectedTab) {
    // Reload the albums in place. setSelectedTab would also switch the main view
    // to the artist page, which kicks the user out of Trash/search/playlist.
    tasks.push(
      getArtistAlbums(selectedTab)
        .then((albums) => {
          if (useLibraryStore.getState().selectedTab === selectedTab) {
            useLibraryStore.setState({ albums });
          }
        })
        .catch((e) => console.error("Failed to refresh artist albums:", e)),
    );
  }
  if (activePlaylistId !== null) {
    tasks.push(
      playlistApi.getSongs(activePlaylistId).then((songs) => {
        // Skip if the user switched playlists meanwhile.
        if (usePlaylistStore.getState().activePlaylistId === activePlaylistId) {
          usePlaylistStore.setState({ activePlaylistSongs: songs });
        }
      }),
    );
  }
  await Promise.all(tasks);
}
