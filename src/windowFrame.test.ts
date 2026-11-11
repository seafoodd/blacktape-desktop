import { beforeEach, describe, expect, it, vi } from "vitest";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isLinuxDesktop, setupWindowFrame } from "./windowFrame.ts";

vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }));

const WEBKITGTK_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const WINDOWS_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0";
const MACOS_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const ANDROID_UA = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36";

// The Node test environment has no DOM (no jsdom in the project), so the root is a minimal stand-in that supports
// the two operations windowFrame.ts uses: dataset writes and toggleAttribute.
interface FakeRoot {
  dataset: Record<string, string>;
  hasAttribute(name: string): boolean;
  toggleAttribute(name: string, force?: boolean): boolean;
}

function makeRoot(): FakeRoot {
  const attributes = new Map<string, string>();
  const dataset: Record<string, string> = {};
  return {
    dataset,
    hasAttribute: (name) => attributes.has(name),
    toggleAttribute(name, force) {
      const on = force ?? !attributes.has(name);
      if (on) attributes.set(name, "");
      else attributes.delete(name);
      return on;
    },
  };
}

function asHtml(root: FakeRoot): HTMLElement {
  return root as unknown as HTMLElement;
}

interface FakeWindow {
  isMaximized: ReturnType<typeof vi.fn>;
  onResized: ReturnType<typeof vi.fn>;
  unlisten: ReturnType<typeof vi.fn>;
  resized: (() => void) | undefined;
}

function installWindow(maximized: boolean): FakeWindow {
  const fake: FakeWindow = {
    isMaximized: vi.fn().mockResolvedValue(maximized),
    unlisten: vi.fn(),
    resized: undefined,
    onResized: vi.fn(),
  };
  fake.onResized.mockImplementation(async (callback: () => void) => {
    fake.resized = callback;
    return fake.unlisten;
  });
  vi.mocked(getCurrentWindow).mockReturnValue(fake as unknown as ReturnType<typeof getCurrentWindow>);
  return fake;
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("isLinuxDesktop", () => {
  it("is true for a WebKitGTK user agent", () => {
    expect(isLinuxDesktop(WEBKITGTK_UA)).toBe(true);
  });

  it("is false for a Windows WebView2 user agent", () => {
    expect(isLinuxDesktop(WINDOWS_UA)).toBe(false);
  });

  it("is false for a macOS user agent", () => {
    expect(isLinuxDesktop(MACOS_UA)).toBe(false);
  });

  it("is false for an Android user agent", () => {
    expect(isLinuxDesktop(ANDROID_UA)).toBe(false);
  });
});

describe("setupWindowFrame", () => {
  it("does nothing on Windows and never touches the Tauri window", async () => {
    installWindow(false);
    const root = makeRoot();

    await setupWindowFrame(asHtml(root), WINDOWS_UA);

    expect(root.dataset.platform).toBeUndefined();
    expect(root.hasAttribute("data-maximized")).toBe(false);
    expect(getCurrentWindow).not.toHaveBeenCalled();
  });

  it("marks the page as linux and omits data-maximized when not maximized", async () => {
    installWindow(false);
    const root = makeRoot();

    await setupWindowFrame(asHtml(root), WEBKITGTK_UA);

    expect(root.dataset.platform).toBe("linux");
    expect(root.hasAttribute("data-maximized")).toBe(false);
  });

  it("sets data-maximized when the window is maximized", async () => {
    installWindow(true);
    const root = makeRoot();

    await setupWindowFrame(asHtml(root), WEBKITGTK_UA);

    expect(root.hasAttribute("data-maximized")).toBe(true);
  });

  it("follows resize events and removes data-maximized when the window is restored", async () => {
    const win = installWindow(false);
    const root = makeRoot();

    await setupWindowFrame(asHtml(root), WEBKITGTK_UA);
    expect(root.hasAttribute("data-maximized")).toBe(false);

    win.isMaximized.mockResolvedValue(true);
    expect(win.resized).toBeTypeOf("function");
    win.resized?.();
    await flush();
    expect(root.hasAttribute("data-maximized")).toBe(true);

    win.isMaximized.mockResolvedValue(false);
    win.resized?.();
    await flush();
    expect(root.hasAttribute("data-maximized")).toBe(false);
  });
});
