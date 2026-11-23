import { useEffect } from "react";
import { useKeymapStore } from "../model/keymapStore.ts";
import { matchesKeyCombo } from "../model/keymapTypes.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { useNavigationStore } from "@/features/navigation/model/navigationStore.ts";

function isUserTyping(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement | null;
  if (!target) return false;

  const tagName = target.tagName.toUpperCase();
  if (tagName === "INPUT" || tagName === "TEXTAREA" || tagName === "SELECT") {
    return true;
  }
  return target.isContentEditable;
}

export function useGlobalShortcuts(): void {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const { keymaps, recordingAction } = useKeymapStore.getState();

      // If user is actively recording a keybind in Settings, let the recorder handle it
      if (recordingAction) return;

      const typing = isUserTyping(e);

      // 1. Search shortcut (e.g. Ctrl + K) - works even if typing
      if (matchesKeyCombo(e, keymaps.search)) {
        e.preventDefault();
        const searchInput = document.querySelector<HTMLInputElement>(
          'input[placeholder*="Search music"]',
        );
        if (searchInput) {
          searchInput.focus();
          searchInput.select();
        }
        return;
      }

      // 2. Play / Pause shortcut (e.g. Space) - only if NOT typing
      if (matchesKeyCombo(e, keymaps.playPause)) {
        if (!typing) {
          e.preventDefault();
          useAudioStore.getState().togglePlay();
          return;
        }
      }

      // 3. Dismiss shortcut (e.g. Escape)
      if (matchesKeyCombo(e, keymaps.dismiss)) {
        if (useModalStore.getState().isOpen) {
          // ModalHost handles dismissible modals, but closing here ensures non-dismissible checks or fallback
          return;
        }

        const activeElem = document.activeElement as HTMLElement | null;
        if (activeElem && (activeElem.tagName === "INPUT" || activeElem.tagName === "TEXTAREA")) {
          e.preventDefault();
          activeElem.blur();
          return;
        }
        return;
      }

      // 4. Navigation Back (e.g. Alt + Left)
      if (matchesKeyCombo(e, keymaps.navBack)) {
        if (!typing) {
          e.preventDefault();
          void useNavigationStore.getState().goBack();
          return;
        }
      }

      // 5. Navigation Forward (e.g. Alt + Right)
      if (matchesKeyCombo(e, keymaps.navForward)) {
        if (!typing) {
          e.preventDefault();
          void useNavigationStore.getState().goForward();
          return;
        }
      }
    };

    const handleMouseUp = (e: MouseEvent) => {
      const { keymaps } = useKeymapStore.getState();
      if (!keymaps.mouseNavEnabled) return;

      // Mouse side button 3 = Back (XBUTTON1)
      if (e.button === 3) {
        e.preventDefault();
        void useNavigationStore.getState().goBack();
        return;
      }

      // Mouse side button 4 = Forward (XBUTTON2)
      if (e.button === 4) {
        e.preventDefault();
        void useNavigationStore.getState().goForward();
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, []);
}
