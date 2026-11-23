import { create } from "zustand";
import {
  DEFAULT_KEYMAPS,
  KeyCombo,
  KeymapAction,
  KeymapConfig,
} from "./keymapTypes.ts";

const STORAGE_KEY = "blacktape_keymaps";
const CHANNEL_NAME = "blacktape_keymaps_channel";

function loadSavedKeymaps(): KeymapConfig {
  if (typeof localStorage === "undefined") return DEFAULT_KEYMAPS;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_KEYMAPS;
    const parsed = JSON.parse(raw);
    return {
      search: parsed.search ?? DEFAULT_KEYMAPS.search,
      playPause: parsed.playPause ?? DEFAULT_KEYMAPS.playPause,
      dismiss: parsed.dismiss ?? DEFAULT_KEYMAPS.dismiss,
      navBack: parsed.navBack ?? DEFAULT_KEYMAPS.navBack,
      navForward: parsed.navForward ?? DEFAULT_KEYMAPS.navForward,
      mouseNavEnabled: parsed.mouseNavEnabled ?? DEFAULT_KEYMAPS.mouseNavEnabled,
    };
  } catch {
    return DEFAULT_KEYMAPS;
  }
}

interface KeymapStoreState {
  keymaps: KeymapConfig;
  recordingAction: KeymapAction | null;
  setKeybind: (action: KeymapAction, combo: KeyCombo) => void;
  setMouseNavEnabled: (enabled: boolean) => void;
  resetToDefaults: () => void;
  startRecording: (action: KeymapAction) => void;
  stopRecording: () => void;
}

function broadcastUpdate(keymaps: KeymapConfig) {
  try {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(keymaps));
    }
    if (typeof BroadcastChannel !== "undefined") {
      const channel = new BroadcastChannel(CHANNEL_NAME);
      channel.postMessage({ type: "KEYMAPS_UPDATED", keymaps });
      channel.close();
    }
  } catch (err) {
    console.error("Failed to persist or broadcast keymaps:", err);
  }
}

export const useKeymapStore = create<KeymapStoreState>((set, get) => ({
  keymaps: loadSavedKeymaps(),
  recordingAction: null,

  setKeybind: (action, combo) => {
    const nextKeymaps: KeymapConfig = {
      ...get().keymaps,
      [action]: combo,
    };
    set({ keymaps: nextKeymaps, recordingAction: null });
    broadcastUpdate(nextKeymaps);
  },

  setMouseNavEnabled: (enabled) => {
    const nextKeymaps: KeymapConfig = {
      ...get().keymaps,
      mouseNavEnabled: enabled,
    };
    set({ keymaps: nextKeymaps });
    broadcastUpdate(nextKeymaps);
  },

  resetToDefaults: () => {
    set({ keymaps: DEFAULT_KEYMAPS, recordingAction: null });
    broadcastUpdate(DEFAULT_KEYMAPS);
  },

  startRecording: (action) => {
    set({ recordingAction: action });
  },

  stopRecording: () => {
    set({ recordingAction: null });
  },
}));

export function initKeymapsSyncListener(): () => void {
  if (typeof BroadcastChannel === "undefined") return () => {};

  const channel = new BroadcastChannel(CHANNEL_NAME);
  channel.onmessage = (e) => {
    if (e.data?.type === "KEYMAPS_UPDATED" && e.data?.keymaps) {
      useKeymapStore.setState({ keymaps: e.data.keymaps });
    }
  };

  const handleStorage = (e: StorageEvent) => {
    if (e.key === STORAGE_KEY && e.newValue) {
      try {
        const parsed = JSON.parse(e.newValue);
        useKeymapStore.setState({ keymaps: parsed });
      } catch {}
    }
  };

  if (typeof window !== "undefined") {
    window.addEventListener("storage", handleStorage);
  }

  return () => {
    channel.close();
    if (typeof window !== "undefined") {
      window.removeEventListener("storage", handleStorage);
    }
  };
}
