import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

/**
 * Turns the OS blur behind the current window on or off. Windows only offers a fixed blur radius,
 * so this is on/off; failures (other platforms, older Windows) only leave the window unblurred.
 */
export function applyWindowBlur(enabled: boolean): void {
  const window = getCurrentWindow();
  const request = enabled
    ? window.setEffects({ effects: [Effect.Blur] })
    : window.clearEffects();
  request.catch((err) => console.error("Failed to change window blur:", err));
}
