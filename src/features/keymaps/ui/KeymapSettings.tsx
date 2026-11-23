import React, { useEffect } from "react";
import { useKeymapStore } from "../model/keymapStore.ts";
import {
  formatKeyCombo,
  KEYMAP_ACTION_LABELS,
  KeymapAction,
} from "../model/keymapTypes.ts";
import styles from "./keymap-settings.module.css";

const ACTIONS: KeymapAction[] = [
  "search",
  "playPause",
  "dismiss",
  "navBack",
  "navForward",
];

const MODIFIER_KEYS = new Set([
  "Control",
  "Shift",
  "Alt",
  "Meta",
  "OS",
  "AltGraph",
]);

export const KeymapSettings: React.FC = () => {
  const {
    keymaps,
    recordingAction,
    setKeybind,
    setMouseNavEnabled,
    resetToDefaults,
    startRecording,
    stopRecording,
  } = useKeymapStore();

  useEffect(() => {
    if (!recordingAction) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      // Ignore pure modifier presses
      if (MODIFIER_KEYS.has(e.key)) return;

      const combo = {
        key: e.key,
        ctrl: e.ctrlKey || undefined,
        alt: e.altKey || undefined,
        shift: e.shiftKey || undefined,
        meta: e.metaKey || undefined,
      };

      setKeybind(recordingAction, combo);
    };

    const handleMouseDown = (e: MouseEvent) => {
      // Clicking outside cancels recording
      const target = e.target as HTMLElement | null;
      if (!target?.closest(`.${styles.shortcutBtnRecording}`)) {
        stopRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("mousedown", handleMouseDown, true);

    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("mousedown", handleMouseDown, true);
    };
  }, [recordingAction, setKeybind, stopRecording]);

  return (
    <div className={styles.keymapList}>
      {ACTIONS.map((action) => {
        const info = KEYMAP_ACTION_LABELS[action];
        const combo = keymaps[action];
        const isRecording = recordingAction === action;

        return (
          <div key={action} className={styles.keymapRow}>
            <div className={styles.keymapInfo}>
              <span className={styles.keymapName}>{info.name}</span>
              <span className={styles.keymapDesc}>{info.description}</span>
            </div>

            <button
              type="button"
              className={`${styles.shortcutBtn} ${isRecording ? styles.shortcutBtnRecording : ""}`}
              onClick={() => {
                if (isRecording) stopRecording();
                else startRecording(action);
              }}
              title="Click to remap shortcut"
            >
              {isRecording ? "Press shortcut…" : formatKeyCombo(combo)}
            </button>
          </div>
        );
      })}

      <div className={styles.keymapRow}>
        <div className={styles.keymapInfo}>
          <span className={styles.keymapName}>Mouse Navigation</span>
          <span className={styles.keymapDesc}>
            Use mouse side buttons (back & forward thumb buttons) to navigate
            history
          </span>
        </div>

        <input
          type="checkbox"
          checked={keymaps.mouseNavEnabled}
          onChange={(e) => setMouseNavEnabled(e.target.checked)}
          aria-label="Toggle mouse navigation"
        />
      </div>

      <div className={styles.footerActions}>
        <button
          type="button"
          className={styles.resetBtn}
          onClick={resetToDefaults}
        >
          Reset to Defaults
        </button>
      </div>
    </div>
  );
};
