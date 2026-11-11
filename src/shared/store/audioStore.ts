import { sameUrl } from "@/shared/lib/remote/urlKey.ts";
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

/** Why the current song could not be played (`playback-error` event of the player). */
export interface PlaybackError {
  title: string;
  message: string;
}

interface AudioState {
  queue: Song[];
  currentSong: Song | null;
  progress: number;
  volume: number;
  isPlaying: boolean;
  /** The current song is a remote stream, not a library file. */
  isStreaming: boolean;
  /** The current stream song is still being opened. */
  streamLoading: boolean;
  shuffleMode: boolean;
  repeatMode: RepeatMode;
  /** Position of the current song in `queue`. */
  queueCursor: number | null;
  /** Index of the current song in the list playback was started from. */
  sourceIndex: number | null;
  /** The current song failed to play; cleared when another song starts or playback resumes. */
  playbackError: PlaybackError | null;

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
  isStreaming: false,
  streamLoading: false,
  shuffleMode: false,
  repeatMode: RepeatMode.Off,
  queueCursor: null,
  sourceIndex: null,
  playbackError: null,
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
    // the song changed while the position was being fetched: that position belongs to the old one
    if (songChanged(get().currentSong, currentSong)) return;
    set({ progress: pos });
  },
}));

type SongRef = { id?: number | null; source_url?: string | null } | null | undefined;

/**
 * True when `next` is a different track than `prev`. A streamed song swapped for its downloaded file (same page
 * url, different id) is the same track: it keeps playing, so the progress must not reset.
 */
export const songChanged = (prev: SongRef, next: SongRef): boolean => {
  if (!prev || !next) return Boolean(prev) !== Boolean(next);
  if (prev.source_url && next.source_url) return !sameUrl(prev.source_url, next.source_url);
  return (prev.id ?? null) !== (next.id ?? null);
};

/** A playback error stays until another song starts or the current one plays again. */
export const keepPlaybackError = (
  error: PlaybackError | null,
  songChanged: boolean,
  isPlaying: boolean,
): PlaybackError | null => (songChanged || isPlaying ? null : error);

if (typeof window !== "undefined") {
  listen("player-state", (event) => {
    const state: any = event.payload;
    const previous = useAudioStore.getState().currentSong;
    // a different song starts at 0 right away, whatever position the event still carries
    const changed = songChanged(previous, state.current_song);
    const { playbackError } = useAudioStore.getState();
    useAudioStore.setState({
      playbackError: keepPlaybackError(playbackError, changed, Boolean(state.is_playing)),
      isPlaying: state.is_playing,
      isStreaming: Boolean(state.streaming),
      streamLoading: Boolean(state.stream_loading),
      currentSong: state.current_song,
      progress: changed ? 0 : state.progress,
      volume: state.volume,
      shuffleMode: state.shuffle_mode,
      repeatMode: state.repeat_mode,
      queue: state.queue,
      queueCursor: state.queue_cursor ?? null,
      sourceIndex: state.source_index ?? null,
    });
  }).catch((e) => console.log(e));

  listen<PlaybackError>("playback-error", (event) => {
    console.error(`[player] cannot play ${event.payload.title}: ${event.payload.message}`);
    useAudioStore.setState({ playbackError: event.payload });
  }).catch((e) => console.log(e));
}
