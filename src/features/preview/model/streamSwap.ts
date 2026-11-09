import { invoke } from "@tauri-apps/api/core";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";

/**
 * When the library gets songs that are queued as streams (downloaded meanwhile), the player swaps the queued
 * streams for the files. The playing song keeps playing its stream. Cheap when nothing matches.
 */
let timer: ReturnType<typeof setTimeout> | undefined;

useLibraryStore.subscribe((state, prev) => {
  if (state.albums === prev.albums) return;
  clearTimeout(timer);
  timer = setTimeout(() => {
    const songs = useLibraryStore
      .getState()
      .albums.flatMap((album) => album.songs)
      .filter((song) => !!song.source_url);
    if (songs.length === 0) return;
    invoke("swap_streams_for_downloads", { songs }).catch((e) =>
      console.error("swap_streams_for_downloads error:", e),
    );
  }, 300);
});
