import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";

export interface ScanProgress {
  phase: "scanning" | "done" | "idle";
  scanned: number;
  total: number;
}

interface ScanStoreState {
  progress: ScanProgress;
  setProgress: (progress: ScanProgress) => void;
  reset: () => void;
}

export const useScanStore = create<ScanStoreState>((set) => ({
  progress: { phase: "idle", scanned: 0, total: 0 },
  setProgress: (progress) => set({ progress }),
  reset: () => set({ progress: { phase: "idle", scanned: 0, total: 0 } }),
}));

export function scanBannerText(progress: ScanProgress): string | null {
  if (progress.phase === "idle") return null;
  if (progress.phase === "done") {
    return `Library scan complete (${progress.scanned} songs)`;
  }
  const pct =
    progress.total > 0
      ? Math.round((progress.scanned / progress.total) * 100)
      : 0;
  return `Scanning library… ${progress.scanned} / ${progress.total} songs (${pct}%)`;
}

let listenerInitialized = false;
let clearTimer: ReturnType<typeof setTimeout> | null = null;

export function initScanProgressListener(): () => void {
  if (listenerInitialized) return () => {};
  listenerInitialized = true;

  const unlistenPromise = listen<ScanProgress>("scan-progress", (event) => {
    if (clearTimer) {
      clearTimeout(clearTimer);
      clearTimer = null;
    }

    if (event.payload.phase === "done") {
      useScanStore.getState().setProgress({
        phase: "done",
        scanned: event.payload.scanned,
        total: event.payload.total,
      });
      clearTimer = setTimeout(() => {
        useScanStore.getState().reset();
        clearTimer = null;
      }, 2000);
    } else {
      useScanStore.getState().setProgress(event.payload);
    }
  });

  return () => {
    unlistenPromise.then((unlisten) => unlisten()).catch(console.error);
    listenerInitialized = false;
  };
}
