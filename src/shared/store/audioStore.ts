import { create } from "zustand";
import type { Song } from "../lib/audio";
import {
  playSong as tauriPlay,
  pause as tauriPause,
  resume as tauriResume,
  seek as tauriSeek,
  getPosition,
} from "../lib/audio";

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
}

export const useAudioStore = create<AudioState>((set, get) => ({
  songs: [],
  currentSong: null,
  progress: 0,
  isPlaying: false,

  setSongs: (songs) => set({ songs }),

  play: async (song) => {
    set({ currentSong: song, isPlaying: true });
    await tauriPlay(song);
  },

  togglePlay: async () => {
    const { isPlaying, currentSong } = get();
    if (!currentSong) return;
    if (isPlaying) {
      await tauriPause();
      set({ isPlaying: false });
    } else {
      await tauriResume();
      set({ isPlaying: true });
    }
  },

  setProgress: (progress) => set({ progress }),

  seek: async (fraction) => {
    set({ progress: fraction });
    await tauriSeek(fraction);
  },

  updateProgress: async () => {
    const { currentSong } = get();
    if (!currentSong) return;
    const pos = await getPosition();
    set({ progress: pos });
  },
}));
