import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

export async function focusOrOpenSettingsWindow(): Promise<void> {
  const existingWindow = await WebviewWindow.getByLabel("settings");

  if (existingWindow) {
    try {
      await existingWindow.unminimize();
      await existingWindow.show();
      await existingWindow.setFocus();
    } catch (err) {
      console.error("Failed to focus settings window:", err);
    }
    return;
  }

  const mainWindow = getCurrentWindow();
  const position = await mainWindow.outerPosition();
  const size = await mainWindow.outerSize();

  const width = 800;
  const height = 600;

  const x = position.x + Math.round((size.width - width) / 2);
  const y = position.y + Math.round((size.height - height) / 2);

  new WebviewWindow("settings", {
    url: "/settings",
    title: "Settings",
    width,
    height,
    x,
    y,
    decorations: false,
    transparent: true,
  });
}
