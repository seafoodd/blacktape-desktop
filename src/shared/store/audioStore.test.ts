import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetProgressSeq, useAudioStore } from "./audioStore.ts";
import * as audioLib from "../lib/audio.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const song = {
  id: 10,
  path: "/music/test.mp3",
  title: "Test Track",
  artists: ["Artist"],
  album_artist: "Artist",
  album: "Album",
  duration_ms: 180_000,
  source: "local" as const,
  canonical_track_slug: "test-track",
  canonical_album_slug: "album",
  quality_tier: "standard" as const,
};

describe("audioStore progress race protection", () => {
  let storageMap: Map<string, string>;

  beforeEach(() => {
    vi.restoreAllMocks();
    storageMap = new Map();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => storageMap.get(key) ?? null,
      setItem: (key: string, val: string) => storageMap.set(key, val),
      removeItem: (key: string) => storageMap.delete(key),
      clear: () => storageMap.clear(),
    });
    resetProgressSeq();
    useAudioStore.setState({
      currentSong: song,
      isPlaying: true,
      progress: 0.1,
    });
  });

  it("updates progress normally when no seek intervened", async () => {
    vi.spyOn(audioLib, "getPosition").mockResolvedValue(0.25);

    await useAudioStore.getState().updateProgress();

    expect(useAudioStore.getState().progress).toBe(0.25);
  });

  it("discards stale in-flight getPosition when seek is called before getPosition resolves", async () => {
    let resolvePosition!: (val: number) => void;
    const delayedPosition = new Promise<number>((res) => {
      resolvePosition = res;
    });

    vi.spyOn(audioLib, "getPosition").mockImplementation(() => delayedPosition);
    vi.spyOn(audioLib, "seek").mockResolvedValue(undefined);

    // 1. Start updateProgress (which fetches getPosition)
    const updatePromise = useAudioStore.getState().updateProgress();

    // 2. User seeks while getPosition is still in flight
    await useAudioStore.getState().seek(0.85);
    expect(useAudioStore.getState().progress).toBe(0.85);

    // 3. Stale position finishes and resolves to old playback position (e.g. 0.12)
    resolvePosition(0.12);
    await updatePromise;

    // 4. Stale position must NOT have rolled back the sought position
    expect(useAudioStore.getState().progress).toBe(0.85);
  });

  it("discards stale in-flight getPosition when setProgress is called", async () => {
    let resolvePosition!: (val: number) => void;
    const delayedPosition = new Promise<number>((res) => {
      resolvePosition = res;
    });

    vi.spyOn(audioLib, "getPosition").mockImplementation(() => delayedPosition);

    const updatePromise = useAudioStore.getState().updateProgress();

    useAudioStore.getState().setProgress(0.6);
    expect(useAudioStore.getState().progress).toBe(0.6);

    resolvePosition(0.15);
    await updatePromise;

    expect(useAudioStore.getState().progress).toBe(0.6);
  });

  it("persists volume to localStorage when setVolume is called", async () => {
    vi.spyOn(audioLib, "setVolume").mockResolvedValue(undefined);
    await useAudioStore.getState().setVolume(0.72);
    expect(useAudioStore.getState().volume).toBe(0.72);
    expect(localStorage.getItem("blacktape_player_volume")).toBe("0.72");
  });
});
