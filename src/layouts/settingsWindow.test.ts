import { beforeEach, describe, expect, it, vi } from "vitest";
import { focusOrOpenSettingsWindow } from "./settingsWindow.ts";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";

vi.mock("@tauri-apps/api/webviewWindow", () => {
  const MockWebviewWindow = vi.fn();
  (MockWebviewWindow as unknown as { getByLabel: unknown }).getByLabel = vi.fn();
  return { WebviewWindow: MockWebviewWindow };
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

describe("focusOrOpenSettingsWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("unminimizes, shows, and focuses an existing settings window", async () => {
    const existingWindow = {
      unminimize: vi.fn().mockResolvedValue(undefined),
      show: vi.fn().mockResolvedValue(undefined),
      setFocus: vi.fn().mockResolvedValue(undefined),
    };
    vi.mocked(WebviewWindow.getByLabel).mockResolvedValue(
      existingWindow as unknown as WebviewWindow,
    );

    await focusOrOpenSettingsWindow();

    expect(existingWindow.unminimize).toHaveBeenCalledTimes(1);
    expect(existingWindow.show).toHaveBeenCalledTimes(1);
    expect(existingWindow.setFocus).toHaveBeenCalledTimes(1);
    expect(WebviewWindow).not.toHaveBeenCalled();
  });

  it("safely catches errors when focusing an existing window without throwing", async () => {
    const existingWindow = {
      unminimize: vi.fn().mockRejectedValue("window.unminimize not allowed"),
      show: vi.fn().mockRejectedValue("window.show not allowed"),
      setFocus: vi.fn().mockRejectedValue("window.set_focus not allowed"),
    };
    vi.mocked(WebviewWindow.getByLabel).mockResolvedValue(
      existingWindow as unknown as WebviewWindow,
    );
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    await expect(focusOrOpenSettingsWindow()).resolves.toBeUndefined();
    expect(consoleErrorSpy).toHaveBeenCalled();
    consoleErrorSpy.mockRestore();
  });

  it("creates a centered settings WebviewWindow when none exists", async () => {
    vi.mocked(WebviewWindow.getByLabel).mockResolvedValue(null);
    vi.mocked(getCurrentWindow).mockReturnValue({
      outerPosition: vi.fn().mockResolvedValue({ x: 100, y: 100 }),
      outerSize: vi.fn().mockResolvedValue({ width: 1200, height: 800 }),
    } as unknown as ReturnType<typeof getCurrentWindow>);

    await focusOrOpenSettingsWindow();

    expect(WebviewWindow).toHaveBeenCalledWith("settings", {
      url: "/settings",
      title: "Settings",
      width: 800,
      height: 600,
      x: 300, // 100 + (1200 - 800) / 2
      y: 200, // 100 + (800 - 600) / 2
      decorations: false,
      transparent: true,
    });
  });
});
