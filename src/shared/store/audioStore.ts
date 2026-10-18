import { create } from "zustand";
import {
  getPosition,
  next as tauriNext,
  pause as tauriPause,
  previous as tauriPrevious,
  seek as tauriSeek,
  setRepeatMode,
  setVolume,
  startPlayback as tauriStartPlayback,
  toggle as tauriToggle,
  toggleShuffle,
} from "../lib/audio";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { RepeatMode, Song } from "@/shared/types.ts";

export type PlaybackContext = "playlist" | "library";

interface AudioState {
  queue: Song[];
  currentSong: Song | null;
  progress: number;
  volume: number;
  isPlaying: boolean;
  shuffleMode: boolean;
  repeatMode: RepeatMode;

  setQueue: (songs: Song[]) => void;
  fetchQueue: () => Promise<void>;
  startPlayback: (
    queue: number[],
    currentIndex: number,
    playbackContext: PlaybackContext,
    contextId?: number,
  ) => Promise<void>;
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

  playbackContext: PlaybackContext | null;
  playbackContextId: number | null;
}

export const useAudioStore = create<AudioState>((set, get) => ({
  queue: [],
  currentSong: null,
  progress: 0,
  volume: 0,
  isPlaying: false,
  shuffleMode: false,
  repeatMode: RepeatMode.Off,
  playbackContext: null,
  playbackContextId: null,

  setQueue: (queue) => set({ queue }),
  fetchQueue: async () => {
    const queue = await invoke<Song[]>("get_queue");
    set({ queue: queue });
  },

  startPlayback: async (
    queue: number[],
    currentIndex: number,
    playbackContext: PlaybackContext,
    contextId?: number,
  ) => {
    // set({ currentSong: song, isPlaying: true });
    await tauriStartPlayback(queue, currentIndex);
    set({ playbackContext, playbackContextId: contextId });
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
    set({ progress: pos });
  },
}));

if (typeof window !== "undefined") {
  listen("player-state", (event) => {
    const state: any = event.payload;
    useAudioStore.setState({
      isPlaying: state.is_playing,
      currentSong: state.current_song,
      progress: state.progress,
      volume: state.volume,
      shuffleMode: state.shuffle_mode,
      repeatMode: state.repeat_mode,
      queue: state.queue,
    });
  }).catch((e) => console.log(e));
}
