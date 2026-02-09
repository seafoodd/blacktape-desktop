import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  mediaControls,
  PlaybackStatus,
  RepeatMode,
} from "tauri-plugin-media-api";
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
    binary += String.fromCharCode(
      ...bytes.slice(i, i + chunkSize)
    );
  }

  return btoa(binary);
}


function App() {
  const [songs, setSongs] = useState<Song[]>([]);
  const [initialized, setInitialized] = useState(false);

  async function ensureMediaSession() {
    if (initialized) return;

    await mediaControls.initialize(
      "blacktape",
      "Blacktape"
    );

    setInitialized(true);
  }

  async function loadSongs() {
    const result = await invoke<Song[]>("scan_music", {
      dir: "C:/Users/seafood/blacktape-lib",
    });
    setSongs(result);
  }

  async function playSong(song: Song) {
    await ensureMediaSession();

    // Start audio playback (rodio)
    await invoke("play_song", { path: song.path });

    // Update OS media session
    await mediaControls.updateNowPlaying(
      {
        title: song.title,
        artist: song.artist,
        album: song.album,
        artworkData: song.cover
          ? bytesToBase64(song.cover)
          : undefined,
      },
      {
        status: PlaybackStatus.Playing,
        position: 0,
        shuffle: false,
        repeatMode: RepeatMode.None,
        playbackRate: 1.0,
      }
    );
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
