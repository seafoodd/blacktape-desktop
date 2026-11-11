import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { checkTools, getToolsStatus, type ToolsStatus } from "@/shared/lib/audio.ts";

interface ToolsState {
  status: ToolsStatus | null;
  /** Loads the current state and follows `tools-status` events; returns the unlisten function. */
  init: () => Promise<() => void>;
  /** "Check for updates" / "Retry". */
  check: () => Promise<void>;
}

/** The newer of two snapshots: the loaded one and events can arrive in either order. */
export function newer(current: ToolsStatus | null, next: ToolsStatus): ToolsStatus {
  return current && current.seq > next.seq ? current : next;
}

export const useToolsStore = create<ToolsState>((set) => ({
  status: null,
  init: async () => {
    const keep = (next: ToolsStatus) => set((state) => ({ status: newer(state.status, next) }));
    const unlisten = await listen<ToolsStatus>("tools-status", (event) => keep(event.payload));
    try {
      keep(await getToolsStatus());
    } catch (err) {
      unlisten();
      throw err;
    }
    return unlisten;
  },
  check: async () => {
    await checkTools();
  },
}));
