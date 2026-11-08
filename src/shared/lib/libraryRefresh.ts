import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { playlistApi } from "@/shared/api/playlists";

/**
 * Reloads everything that shows song metadata after a library change:
 * artist list, the opened artist's albums and the opened playlist's songs.
 * (The player's current song and queue are pushed by the backend.)
 */
export async function refreshLibrary(): Promise<void> {
  const { selectedTab, fetchTabs, setSelectedTab } = useLibraryStore.getState();
  const { activePlaylistId } = usePlaylistStore.getState();

  const tasks: Promise<unknown>[] = [fetchTabs()];
  if (selectedTab) tasks.push(Promise.resolve(setSelectedTab(selectedTab)));
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
