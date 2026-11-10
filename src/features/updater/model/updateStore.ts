import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "installing"
  | "error";

interface UpdateState {
  status: UpdateStatus;
  /** Version of the available update, without a leading "v". */
  version: string | null;
  notes: string | null;
  downloaded: number;
  /** Size of the update in bytes, when the server reports it. */
  total: number | null;
  error: string | null;
  update: Update | null;
  /** A silent check (on startup) keeps network errors out of the interface. */
  checkForUpdate: (options?: { silent?: boolean }) => Promise<void>;
  /** Downloads and installs the found update, then restarts the app. */
  installUpdate: () => Promise<void>;
}

const BUSY: UpdateStatus[] = ["checking", "downloading", "installing"];

export const useUpdateStore = create<UpdateState>((set, get) => ({
  status: "idle",
  version: null,
  notes: null,
  downloaded: 0,
  total: null,
  error: null,
  update: null,

  checkForUpdate: async ({ silent = false } = {}) => {
    const { status } = get();
    if (BUSY.includes(status)) return;
    set({ status: "checking", error: null });
    try {
      const update = await check();
      if (!update) {
        set({ status: "upToDate", update: null, version: null, notes: null });
        return;
      }
      set({
        status: "available",
        update,
        version: update.version,
        notes: update.body?.trim() || null,
      });
    } catch (e) {
      console.warn("[updater] check failed:", e);
      set(
        silent
          ? { status: status === "available" ? "available" : "idle" }
          : { status: "error", error: String(e) },
      );
    }
  },

  installUpdate: async () => {
    const { update, status } = get();
    if (!update || BUSY.includes(status)) return;
    set({ status: "downloading", downloaded: 0, total: null, error: null });
    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            set({ total: event.data.contentLength ?? null });
            break;
          case "Progress":
            set((s) => ({ downloaded: s.downloaded + event.data.chunkLength }));
            break;
          case "Finished":
            set({ status: "installing" });
            break;
        }
      });
      // On Windows the installer closes the app itself; elsewhere it has to restart into the new version.
      await relaunch();
    } catch (e) {
      console.error("[updater] install failed:", e);
      set({ status: "error", error: String(e) });
    }
  },
}));

/** Download progress in whole percent, or null while the size is unknown. */
export function downloadPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((downloaded / total) * 100)));
}

/** Text for the install button in its current state. */
export function installLabel(
  status: UpdateStatus,
  version: string | null,
  downloaded: number,
  total: number | null,
): string {
  if (status === "downloading") {
    const percent = downloadPercent(downloaded, total);
    return percent === null ? "Downloading…" : `Downloading ${percent}%`;
  }
  if (status === "installing") return "Installing…";
  return version ? `Install v${version} and restart` : "Install and restart";
}
