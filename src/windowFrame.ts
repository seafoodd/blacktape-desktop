import { getCurrentWindow } from "@tauri-apps/api/window";

/** Whether the webview runs on desktop Linux (WebKitGTK: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/..."). */
export function isLinuxDesktop(userAgent: string): boolean {
  return /\bLinux\b/.test(userAgent) && !/\bAndroid\b/.test(userAgent);
}

/**
 * On Linux, marks the page so globals.css draws the window's edge, and drops the edge while the window is
 * maximized. Windows and macOS draw their own frame around a window without decorations.
 */
export async function setupWindowFrame(
  root: HTMLElement = document.documentElement,
  userAgent: string = navigator.userAgent,
): Promise<void> {
  if (!isLinuxDesktop(userAgent)) return;
  root.dataset.platform = "linux";
  const window = getCurrentWindow();
  const update = async () => {
    root.toggleAttribute("data-maximized", await window.isMaximized());
  };
  await update();
  await window.onResized(() => {
    update().catch(console.error);
  });
}
