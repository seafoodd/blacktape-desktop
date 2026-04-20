import { useTheme } from "./shared/providers/theme-provider";
import PlayerControls from "./components/player-controls/PlayerControls";
import { pickFolder } from "./shared/lib/dialog";
import { fetchState, scanMusic } from "./shared/lib/audio";
import { useEffect } from "react";
import styles from "./app.module.css";
import LeftSidebar from "./components/left-sidebar/LeftSidebar.tsx";

import ArtistAlbums from "@/components/artist-albums/ArtistAlbums.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";

function App() {
  const { theme, toggleTheme } = useTheme();
  const { fetchTabs } = useLibraryStore();

  async function handlePickFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    const loadedSongs = await scanMusic(dir);
    console.log("loadedSongs: ", loadedSongs[0]);
    await fetchTabs();
  }

  useEffect(() => {
    fetchState();
  }, []);

  return (
    <main className={styles.app}>
      <header className={styles.header}>
        <button onClick={handlePickFolder}>Select Music Folder</button>
        <button onClick={toggleTheme}>
          {theme === "light" ? "Switch to Dark" : "Switch to Light"}
        </button>
      </header>

      <div className={styles.layout}>
        <LeftSidebar />

        {/* Main Content */}
        <main className={styles.main}>
          <ArtistAlbums />
        </main>
      </div>

      {/* Footer */}
      <footer className={styles.footer}>
        <PlayerControls />
      </footer>
    </main>
  );
}

export default App;
