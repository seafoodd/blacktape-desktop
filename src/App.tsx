import { useTheme } from "./shared/providers/theme-provider";
import PlayerControls from "./components/player-controls/PlayerControls";
import { pickFolder } from "./shared/lib/dialog";
import { useAudioStore } from "./shared/store/audioStore";
import { scanMusic } from "./shared/lib/audio";

function App() {
  const { theme, toggleTheme } = useTheme();
  const { songs, setSongs, currentSong, play } = useAudioStore();
  async function handlePickFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    setSongs([]);
    const loadedSongs = await scanMusic(dir);
    setSongs(loadedSongs);
  }

  return (
    <main className="container">
      <h1>Welcome to Blacktape</h1>

      <button onClick={handlePickFolder}>Select music folder</button>

      <button onClick={toggleTheme}>
        {theme === "light" ? "Switch to Dark" : "Switch to Light"}
      </button>

      {currentSong && <PlayerControls />}

      <ul style={{ marginTop: 20 }}>
        {songs.map((song, i) => (
          <li
            key={i}
            style={{ cursor: "pointer", marginBottom: 8 }}
            onClick={() => play(song)}
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
