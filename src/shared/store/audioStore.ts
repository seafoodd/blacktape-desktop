import { create } from "zustand";
import { setVolume, Song } from "../lib/audio";
import {
  startPlayback as tauriStartPlayback,
  seek as tauriSeek,
  toggle as tauriToggle,
  pause as tauriPause,
  next as tauriNext,
  previous as tauriPrevious,
  getPosition,
} from "../lib/audio";
import { listen } from "@tauri-apps/api/event";

interface AudioState {
  songs: Song[];
  currentSong: Song | null;
  progress: number;
  volume: number;
  isPlaying: boolean;

  setSongs: (songs: Song[]) => void;
  startPlayback: (
    songId: number,
    queue: number[],
    history: number[],
  ) => Promise<void>;
  togglePlay: () => Promise<void>;
  setProgress: (value: number) => void;
  setVolume: (value: number) => void;
  seek: (fraction: number) => Promise<void>;
  next: () => Promise<void>;
  previous: () => Promise<void>;
  updateProgress: () => Promise<void>;
  pause: () => Promise<void>;
}

export const useAudioStore = create<AudioState>((set, get) => ({
  songs: [],
  currentSong: null,
  progress: 0,
  volume: 0,
  isPlaying: false,

  setSongs: (songs) => set({ songs }),

  startPlayback: async (songId: number, queue: number[], history: number[]) => {
    // set({ currentSong: song, isPlaying: true });
    await tauriStartPlayback(songId, queue, history);
  },

  togglePlay: async () => {
    // const { currentSong } = get();
    // if (!currentSong) return;
    console.log("togglePlay");
    await tauriToggle();
    // const isPaused = await tauriIsPaused();
    // set({ isPlaying: !isPaused });
  },

  pause: async () => {
    await tauriPause();
  },

  setProgress: (progress) => set({ progress }),

  setVolume: async (volume) => {
    set({ volume });
    await setVolume(volume);
  },

  seek: async (progress) => {
    set({ progress });
    await tauriSeek(progress);
  },

  next: async () => {
    await tauriNext();
  },

  previous: async () => {
    await tauriPrevious();
  },

  updateProgress: async () => {
    const { currentSong, isPlaying } = get();
    if (!currentSong || !isPlaying) return;
    const pos = await getPosition();
    console.log("updated progress", pos);
    set({ progress: pos });
  },
}));

if (typeof window !== "undefined") {
  listen("player-state", (event) => {
    const state: any = event.payload;
    console.log("PLAYER STATEEE", state);
    useAudioStore.setState({
      isPlaying: state.is_playing,
      currentSong: state.current_song,
      progress: state.progress,
      volume: state.volume,
    });
  });
}
