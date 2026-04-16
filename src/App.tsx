import { useTheme } from "./shared/providers/theme-provider";
import PlayerControls from "./components/player-controls/PlayerControls";
import { pickFolder } from "./shared/lib/dialog";
import { useAudioStore } from "./shared/store/audioStore";
import { scanMusic } from "./shared/lib/audio";
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "./app.module.css";
import LeftSidebar from "./components/left-sidebar/LeftSidebar.tsx";

function App() {
  const { theme, toggleTheme } = useTheme();
  const { songs, setSongs, play } = useAudioStore();

  async function handlePickFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    setSongs([]);
    const loadedSongs = await scanMusic(dir);
    console.log("loadedSongs: ", loadedSongs[0]);
    setSongs(loadedSongs);
  }

  useEffect(() => {
    const handleUnload = () => {
      invoke("stop");
    };

    window.addEventListener("beforeunload", handleUnload);
    return () => window.removeEventListener("beforeunload", handleUnload);
  }, []);

  return (
    <div className={styles.app}>
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
          <ul className={styles.songs}>
            {songs.map((song, i) => (
              <li className={styles.song} key={i} onClick={() => play(song)}>
                <strong>{song.title}</strong> — {song.artist}
                <br />
                <small>{song.album}</small>
              </li>
            ))}
          </ul>
        </main>
      </div>

      {/* Footer with player controls */}
      <footer className={styles.footer}>
        <PlayerControls />
      </footer>
    </div>
  );
}

export default App;
