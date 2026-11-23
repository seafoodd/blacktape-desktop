export interface KeyCombo {
  key: string;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  meta?: boolean;
}

export interface KeymapConfig {
  search: KeyCombo;
  playPause: KeyCombo;
  dismiss: KeyCombo;
  navBack: KeyCombo;
  navForward: KeyCombo;
  mouseNavEnabled: boolean;
}

export const DEFAULT_KEYMAPS: KeymapConfig = {
  search: { key: "k", ctrl: true },
  playPause: { key: " " },
  dismiss: { key: "Escape" },
  navBack: { key: "ArrowLeft", alt: true },
  navForward: { key: "ArrowRight", alt: true },
  mouseNavEnabled: true,
};

export type KeymapAction = "search" | "playPause" | "dismiss" | "navBack" | "navForward";

export const KEYMAP_ACTION_LABELS: Record<KeymapAction, { name: string; description: string }> = {
  search: {
    name: "Focus Search Bar",
    description: "Focus and select the main search input to find music online or in library",
  },
  playPause: {
    name: "Play / Pause",
    description: "Toggle playback of current track (when not typing in a text field)",
  },
  dismiss: {
    name: "Dismiss / Close",
    description: "Close open modals, cancel search suggestions, or clear active selections",
  },
  navBack: {
    name: "Navigate Back",
    description: "Go back to the previously viewed screen",
  },
  navForward: {
    name: "Navigate Forward",
    description: "Go forward to the next screen in history",
  },
};

export function formatKeyCombo(combo: KeyCombo | undefined): string {
  if (!combo || !combo.key) return "Unbound";

  const parts: string[] = [];

  const isMac =
    typeof navigator !== "undefined" &&
    /Mac|iPod|iPhone|iPad/.test(navigator.platform);

  if (combo.ctrl) parts.push(isMac ? "Control" : "Ctrl");
  if (combo.alt) parts.push(isMac ? "Option" : "Alt");
  if (combo.shift) parts.push("Shift");
  if (combo.meta) parts.push(isMac ? "Cmd" : "Win");

  let keyLabel = combo.key;
  if (keyLabel === " ") {
    keyLabel = "Space";
  } else if (keyLabel === "Escape") {
    keyLabel = "Esc";
  } else if (keyLabel === "ArrowLeft") {
    keyLabel = "←";
  } else if (keyLabel === "ArrowRight") {
    keyLabel = "→";
  } else if (keyLabel === "ArrowUp") {
    keyLabel = "↑";
  } else if (keyLabel === "ArrowDown") {
    keyLabel = "↓";
  } else if (keyLabel.length === 1) {
    keyLabel = keyLabel.toUpperCase();
  }

  parts.push(keyLabel);
  return parts.join(" + ");
}

export function matchesKeyCombo(e: KeyboardEvent, combo: KeyCombo | undefined): boolean {
  if (!combo || !combo.key) return false;

  const requiresCtrl = Boolean(combo.ctrl);
  const requiresAlt = Boolean(combo.alt);
  const requiresShift = Boolean(combo.shift);
  const requiresMeta = Boolean(combo.meta);

  const eventCtrl = Boolean(e.ctrlKey);
  const eventAlt = Boolean(e.altKey);
  const eventShift = Boolean(e.shiftKey);
  const eventMeta = Boolean(e.metaKey);

  // If ctrl was specified, check either ctrlKey or (on Mac) metaKey
  if (requiresCtrl && !eventCtrl && !eventMeta) return false;
  if (!requiresCtrl && (eventCtrl || eventMeta)) return false;

  if (requiresAlt !== eventAlt) return false;
  if (requiresShift !== eventShift) return false;
  if (requiresMeta !== eventMeta && !requiresCtrl) return false;

  if (combo.key === " " || combo.key.toLowerCase() === "space") {
    return e.key === " " || e.code === "Space";
  }

  return e.key.toLowerCase() === combo.key.toLowerCase();
}
