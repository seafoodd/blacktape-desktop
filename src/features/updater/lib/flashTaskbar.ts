import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";

/**
 * Makes the app's taskbar button flash (Windows) / bounce the dock icon (macOS) until the window is focused.
 * Does nothing visible when the window already has the focus. Errors are only logged.
 */
export async function flashTaskbar(): Promise<void> {
  try {
    await getCurrentWindow().requestUserAttention(UserAttentionType.Informational);
  } catch (e) {
    console.warn("[updater] could not request attention:", e);
  }
}
