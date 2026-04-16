import { create } from "zustand";
import type { Song } from "../lib/audio";
import {
  playSong as tauriPlay,
  seek as tauriSeek,
  toggle as tauriToggle,
  pause as tauriPause,
  getPosition,
} from "../lib/audio";
import { listen } from "@tauri-apps/api/event";

interface AudioState {
  songs: Song[];
  currentSong: Song | null;
  progress: number;
  isPlaying: boolean;

  setSongs: (songs: Song[]) => void;
  play: (song: Song) => Promise<void>;
  togglePlay: () => Promise<void>;
  setProgress: (value: number) => void;
  seek: (fraction: number) => Promise<void>;
  updateProgress: () => Promise<void>;
  pause: () => Promise<void>;
}

export const useAudioStore = create<AudioState>((set, get) => ({
  songs: [],
  currentSong: null,
  progress: 0,
  isPlaying: false,

  setSongs: (songs) => set({ songs }),

  play: async (song) => {
    // set({ currentSong: song, isPlaying: true });
    await tauriPlay(song.id);
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

  seek: async (fraction) => {
    set({ progress: fraction });
    tauriSeek(fraction);
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
    });
  });
}
