import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  formatKeyCombo,
  matchesKeyCombo,
  DEFAULT_KEYMAPS,
} from "./keymapTypes.ts";
import { useKeymapStore } from "./keymapStore.ts";

describe("keymapTypes and formatKeyCombo", () => {
  it("formats standard key combos nicely", () => {
    expect(formatKeyCombo({ key: "k", ctrl: true })).toBe("Ctrl + K");
    expect(formatKeyCombo({ key: " " })).toBe("Space");
    expect(formatKeyCombo({ key: "Escape" })).toBe("Esc");
    expect(formatKeyCombo({ key: "ArrowLeft", alt: true })).toBe("Alt + ←");
    expect(formatKeyCombo({ key: "ArrowRight", alt: true })).toBe("Alt + →");
    expect(formatKeyCombo(undefined)).toBe("Unbound");
  });

  describe("matchesKeyCombo", () => {
    it("matches Ctrl + K", () => {
      const event = {
        key: "k",
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(matchesKeyCombo(event, { key: "k", ctrl: true })).toBe(true);
      expect(matchesKeyCombo(event, { key: "k", ctrl: false })).toBe(false);
      expect(matchesKeyCombo(event, { key: "p", ctrl: true })).toBe(false);
    });

    it("matches Space key", () => {
      const spaceEvent = {
        key: " ",
        code: "Space",
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(matchesKeyCombo(spaceEvent, { key: " " })).toBe(true);

      const modifiedSpaceEvent = {
        key: " ",
        code: "Space",
        ctrlKey: true,
        altKey: false,
        shiftKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(matchesKeyCombo(modifiedSpaceEvent, { key: " " })).toBe(false);
    });

    it("matches Escape key", () => {
      const escEvent = {
        key: "Escape",
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(matchesKeyCombo(escEvent, { key: "Escape" })).toBe(true);
    });

    it("matches Alt + ArrowLeft", () => {
      const altLeftEvent = {
        key: "ArrowLeft",
        ctrlKey: false,
        altKey: true,
        shiftKey: false,
        metaKey: false,
      } as KeyboardEvent;
      expect(
        matchesKeyCombo(altLeftEvent, { key: "ArrowLeft", alt: true }),
      ).toBe(true);
      expect(
        matchesKeyCombo(altLeftEvent, { key: "ArrowRight", alt: true }),
      ).toBe(false);
    });
  });
});

describe("keymapStore", () => {
  beforeEach(() => {
    vi.stubGlobal("localStorage", {
      getItem: vi.fn(() => null),
      setItem: vi.fn(),
      removeItem: vi.fn(),
      clear: vi.fn(),
    });
    useKeymapStore.getState().resetToDefaults();
  });

  it("initializes with default keymaps", () => {
    const { keymaps } = useKeymapStore.getState();
    expect(keymaps.search).toEqual(DEFAULT_KEYMAPS.search);
    expect(keymaps.playPause).toEqual(DEFAULT_KEYMAPS.playPause);
    expect(keymaps.dismiss).toEqual(DEFAULT_KEYMAPS.dismiss);
    expect(keymaps.mouseNavEnabled).toBe(true);
  });

  it("updates a keybind and persists", () => {
    useKeymapStore.getState().setKeybind("search", { key: "f", ctrl: true });
    expect(useKeymapStore.getState().keymaps.search).toEqual({
      key: "f",
      ctrl: true,
    });
  });

  it("toggles mouse side button navigation", () => {
    useKeymapStore.getState().setMouseNavEnabled(false);
    expect(useKeymapStore.getState().keymaps.mouseNavEnabled).toBe(false);
    useKeymapStore.getState().setMouseNavEnabled(true);
    expect(useKeymapStore.getState().keymaps.mouseNavEnabled).toBe(true);
  });

  it("manages recording state", () => {
    expect(useKeymapStore.getState().recordingAction).toBeNull();
    useKeymapStore.getState().startRecording("playPause");
    expect(useKeymapStore.getState().recordingAction).toBe("playPause");
    useKeymapStore.getState().stopRecording();
    expect(useKeymapStore.getState().recordingAction).toBeNull();
  });

  it("resets to defaults", () => {
    useKeymapStore.getState().setKeybind("search", { key: "s", alt: true });
    useKeymapStore.getState().resetToDefaults();
    expect(useKeymapStore.getState().keymaps.search).toEqual(
      DEFAULT_KEYMAPS.search,
    );
  });
});
