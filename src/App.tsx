import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type Song = {
  path: string;
  title: string;
  artist: string;
  album: string;
  cover?: number[] | null;
};

function bytesToBase64(bytes: number[]): string {
  let binary = "";
  const chunkSize = 0x8000; // 32KB chunks

  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.slice(i, i + chunkSize));
  }

  return btoa(binary);
}

function App() {
  const [songs, setSongs] = useState<Song[]>([]);
  const [initialized, setInitialized] = useState(false);
  const [progress, setProgress] = useState(0);
  const [currentSong, setCurrentSong] = useState<Song | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    if (!currentSong) return;
    const interval = setInterval(async () => {
      if (isDragging) return;
      const pos = await invoke<number>("get_position");
      setProgress(pos);
    }, 500);

    return () => clearInterval(interval);
  }, [currentSong, isDragging]);

  async function loadSongs() {
    const result = await invoke<Song[]>("scan_music", {
      dir: "C:/Users/seafood/blacktape-lib",
    });
    setSongs(result);
  }

  async function playSong(song: Song) {
    setCurrentSong(song);
    setIsPlaying(true);
    await invoke("play_song", { path: song.path });
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
      <button onClick={pickFolder}>Select music folder</button>
      <button onClick={loadSongs}>Scan music folder</button>
      {currentSong && (
        <div className="player-controls">
          <button
            onClick={async () => {
              if (isPlaying) {
                await invoke("pause");
                setIsPlaying(false);
              } else if (currentSong) {
                await invoke("resume");
                setIsPlaying(true);
              }
            }}
          >
            {isPlaying ? "Pause" : "Play"}
          </button>

          <input
            type="range"
            min={0}
            max={1000}
            value={progress * 1000}
            onChange={(e) => setProgress(Number(e.target.value) / 1000)}
            onMouseDown={() => setIsDragging(true)}
            onMouseUp={async (e) => {
              setIsDragging(false);
              const val = Number(e.currentTarget.value) / 1000;
              await invoke("seek", { fraction: val });
            }}
            onTouchStart={() => setIsDragging(true)}
            onTouchEnd={async (e) => {
              setIsDragging(false);
              const val = Number(e.currentTarget.value) / 1000;
              await invoke("seek", { fraction: val });
            }}
            style={{ width: 300 }}
          />
        </div>
      )}

      <ul style={{ marginTop: 20 }}>
        {songs.map((song, i) => (
          <li
            key={i}
            style={{ cursor: "pointer", marginBottom: 8 }}
            onClick={() => playSong(song)}
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
