import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@/shared/lib/audio.ts", () => ({ getToolsStatus: vi.fn(), checkTools: vi.fn() }));

import { listen } from "@tauri-apps/api/event";
import { getToolsStatus, type ToolStatus, type ToolsStatus } from "@/shared/lib/audio.ts";
import { newer, useToolsStore } from "./toolsStore.ts";

const ready: ToolStatus = {
  phase: "ready",
  usable: true,
  version: null,
  source: "app",
  size: null,
  downloaded: 0,
  total: null,
  error: null,
};

const snapshot = (seq: number): ToolsStatus => ({ seq, ytDlp: ready, deno: ready, ffmpeg: ready });

describe("newer", () => {
  it("keeps the snapshot with the higher seq", () => {
    expect(newer(snapshot(5), snapshot(3)).seq).toBe(5);
    expect(newer(snapshot(3), snapshot(5)).seq).toBe(5);
    expect(newer(null, snapshot(1)).seq).toBe(1);
  });
});

describe("init", () => {
  it("does not let a slow snapshot overwrite a newer event", async () => {
    let emit: (s: ToolsStatus) => void = () => {};
    vi.mocked(listen).mockImplementation(async (_name, handler) => {
      emit = (s) => handler({ payload: s } as never);
      return () => {};
    });
    vi.mocked(getToolsStatus).mockImplementation(async () => {
      emit(snapshot(7)); // the event arrives before the older snapshot's reply
      return snapshot(2);
    });
    useToolsStore.setState({ status: null });
    await useToolsStore.getState().init();
    expect(useToolsStore.getState().status?.seq).toBe(7);
  });

  it("removes the listener when the snapshot cannot be loaded", async () => {
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValue(unlisten);
    vi.mocked(getToolsStatus).mockRejectedValue(new Error("no backend"));
    await expect(useToolsStore.getState().init()).rejects.toThrow("no backend");
    expect(unlisten).toHaveBeenCalled();
  });
});
