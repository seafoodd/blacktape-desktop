import { create } from "zustand";
import {
  getPosition,
  next as tauriNext,
  pause as tauriPause,
  previous as tauriPrevious,
  RepeatMode,
  seek as tauriSeek, setRepeatMode,
  setVolume,
  Song,
  startPlayback as tauriStartPlayback,
  toggle as tauriToggle,
  toggleShuffle
} from "../lib/audio";
import { listen } from "@tauri-apps/api/event";

interface AudioState {
  songs: Song[];
  currentSong: Song | null;
  progress: number;
  volume: number;
  isPlaying: boolean;
  shuffleMode: boolean;
  repeatMode: RepeatMode;

  setSongs: (songs: Song[]) => void;
  startPlayback: (queue: number[], current_index: number) => Promise<void>;
  togglePlay: () => Promise<void>;
  toggleShuffle: () => Promise<void>;
  cycleRepeatMode: () => Promise<void>;
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
  shuffleMode: false,
  repeatMode: RepeatMode.Off,

  setSongs: (songs) => set({ songs }),

  startPlayback: async (queue: number[], current_index: number) => {
    // set({ currentSong: song, isPlaying: true });
    await tauriStartPlayback(queue, current_index);
  },

  togglePlay: async () => {
    // const { currentSong } = get();
    // if (!currentSong) return;
    console.log("togglePlay");
    await tauriToggle();
    // const isPaused = await tauriIsPaused();
    // set({ isPlaying: !isPaused });
  },

  toggleShuffle: async () => {
    set((state) => ({
      shuffleMode: !state.shuffleMode,
    }));
    toggleShuffle();
  },

  cycleRepeatMode: async () => {
    const current = get().repeatMode;
    const repeatOrder = Object.values(RepeatMode);
    const nextMode =
      repeatOrder[(repeatOrder.indexOf(current) + 1) % repeatOrder.length];
    set({ repeatMode: nextMode });
    setRepeatMode(nextMode);
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
      shuffleMode: state.shuffle_mode,
      repeatMode: state.repeat_mode
    });
  });
}
