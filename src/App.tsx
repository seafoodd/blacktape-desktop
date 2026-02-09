import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { open } from "@tauri-apps/plugin-dialog";

type Song = {
  path: string;
  title: string;
  artist: string;
  album: string;
  cover?: number[] | null;
};

function App() {
  const [songs, setSongs] = useState<Song[]>([]);

  async function loadSongs() {
    const result = await invoke<Song[]>("scan_music", {
      dir: "C:/Users/seafood/blacktape-lib"
    });
    setSongs(result);
  }

  async function playSong(path: string) {
    await invoke("play_song", { path });
  }

  async function pickFolder() {
    const dir = await open({
      directory: true,
      multiple: false,
    });

    if (typeof dir === "string") {
      const result = await invoke<Song[]>("scan_music", { dir });
      setSongs(result);
    }
  }


  return (
    <main className="container">
      <h1>Welcome to Blacktape</h1>

      <button onClick={pickFolder}>
        Select music folder
      </button>
      <button onClick={loadSongs}>Scan music folder</button>

      <ul style={{ marginTop: 20 }}>
        {songs.map((song, i) => (
          <li
            key={i}
            style={{ cursor: "pointer", marginBottom: 8 }}
            onClick={() => playSong(song.path)}
          >
            <strong>{song.title}</strong> — {song.artist}
            <br />
            <small>{song.album}</small>
          </li>
        ))}
      </ul>
    </main>
  );
}

export default App;
