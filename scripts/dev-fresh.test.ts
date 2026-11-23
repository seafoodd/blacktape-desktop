import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  getFreshDirs,
  cleanFreshDirs,
  seedFreshToolsAndCookies,
  isWorkspaceDevProcess,
  writePidFile,
  readPidFile,
  removePidFile,
  PID_FILE_NAME,
} from "./dev-fresh.js";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

describe("getFreshDirs", () => {
  it("returns Windows AppData and LocalAppData paths", () => {
    const dirs = getFreshDirs("win32", {
      APPDATA: "C:\\Users\\Test\\AppData\\Roaming",
      LOCALAPPDATA: "C:\\Users\\Test\\AppData\\Local",
    });
    expect(dirs).toContain(
      "C:\\Users\\Test\\AppData\\Roaming\\dev.seafood.blacktape.fresh",
    );
    expect(dirs).toContain(
      "C:\\Users\\Test\\AppData\\Local\\dev.seafood.blacktape.fresh",
    );
  });

  it("returns macOS Application Support path", () => {
    const dirs = getFreshDirs("darwin", {}, "/Users/test");
    expect(dirs).toEqual([
      "/Users/test/Library/Application Support/dev.seafood.blacktape.fresh",
    ]);
  });

  it("returns Linux XDG paths", () => {
    const dirs = getFreshDirs(
      "linux",
      {
        XDG_DATA_HOME: "/home/test/.local/share",
        XDG_CONFIG_HOME: "/home/test/.config",
      },
      "/home/test",
    );
    expect(dirs).toContain("/home/test/.local/share/dev.seafood.blacktape.fresh");
    expect(dirs).toContain("/home/test/.config/dev.seafood.blacktape.fresh");
  });
});

describe("cleanFreshDirs", () => {
  it("removes directories if they exist", () => {
    const existsSpy = vi.spyOn(fs, "existsSync").mockReturnValue(true);
    const rmSpy = vi.spyOn(fs, "rmSync").mockImplementation(() => {});

    cleanFreshDirs("win32", {
      APPDATA: "C:\\Roaming",
      LOCALAPPDATA: "C:\\Local",
    });

    expect(existsSpy).toHaveBeenCalled();
    expect(rmSpy).toHaveBeenCalledTimes(2);

    existsSpy.mockRestore();
    rmSpy.mockRestore();
  });
});

describe("isWorkspaceDevProcess", () => {
  const workspaceRoot = "C:\\Dev\\blacktape-desktop";

  it("returns true for binaries within workspace src-tauri/target", () => {
    expect(
      isWorkspaceDevProcess(
        "C:\\Dev\\blacktape-desktop\\src-tauri\\target\\debug\\blacktape-desktop.exe",
        workspaceRoot,
      ),
    ).toBe(true);

    expect(
      isWorkspaceDevProcess(
        "C:/Dev/blacktape-desktop/src-tauri/target/release/blacktape-desktop.exe",
        workspaceRoot,
      ),
    ).toBe(true);

    const linuxWorkspace =
      "/home/runner/work/blacktape-desktop/blacktape-desktop";
    expect(
      isWorkspaceDevProcess(
        "/home/runner/work/blacktape-desktop/blacktape-desktop/src-tauri/target/debug/blacktape-desktop",
        linuxWorkspace,
      ),
    ).toBe(true);
  });

  it("returns false for installed or release binaries outside workspace", () => {
    expect(
      isWorkspaceDevProcess(
        "Y:\\tools\\Blacktape\\blacktape-desktop.exe",
        workspaceRoot,
      ),
    ).toBe(false);

    expect(
      isWorkspaceDevProcess(
        "C:\\Program Files\\Blacktape\\blacktape-desktop.exe",
        workspaceRoot,
      ),
    ).toBe(false);

    expect(
      isWorkspaceDevProcess(
        "C:\\Users\\seafood\\AppData\\Local\\Programs\\Blacktape\\blacktape-desktop.exe",
        workspaceRoot,
      ),
    ).toBe(false);

    const linuxWorkspace =
      "/home/runner/work/blacktape-desktop/blacktape-desktop";
    expect(
      isWorkspaceDevProcess("/usr/bin/blacktape-desktop", linuxWorkspace),
    ).toBe(false);
  });

  it("returns false for invalid, null or empty paths", () => {
    expect(isWorkspaceDevProcess("", workspaceRoot)).toBe(false);
    expect(isWorkspaceDevProcess(null as unknown as string, workspaceRoot)).toBe(
      false,
    );
    expect(
      isWorkspaceDevProcess(undefined as unknown as string, workspaceRoot),
    ).toBe(false);
  });
});

describe("pidFile helpers", () => {
  const tmpDir = path.join(os.tmpdir(), "blacktape-test-" + Date.now());

  beforeEach(() => {
    fs.mkdirSync(tmpDir, { recursive: true });
  });

  afterEach(() => {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_) {}
  });

  it("writes, reads, and removes PID file correctly", () => {
    const pidInfo = {
      runnerPid: 12345,
      childPid: 67890,
      startedAt: "2026-11-23T14:00:00.000Z",
    };

    expect(readPidFile(tmpDir)).toBeNull();

    writePidFile(pidInfo, tmpDir);
    expect(fs.existsSync(path.join(tmpDir, PID_FILE_NAME))).toBe(true);

    const read = readPidFile(tmpDir);
    expect(read).toEqual(pidInfo);

    removePidFile(tmpDir);
    expect(fs.existsSync(path.join(tmpDir, PID_FILE_NAME))).toBe(false);
    expect(readPidFile(tmpDir)).toBeNull();
  });
});

describe("seedFreshToolsAndCookies", () => {
  const tmpBase = path.join(os.tmpdir(), "blacktape-seed-" + Date.now());
  const appData = path.join(tmpBase, "AppData", "Roaming");
  const localAppData = path.join(tmpBase, "AppData", "Local");

  beforeEach(() => {
    fs.mkdirSync(path.join(appData, "dev.seafood.blacktape"), {
      recursive: true,
    });
    fs.mkdirSync(path.join(localAppData, "dev.seafood.blacktape"), {
      recursive: true,
    });
    fs.writeFileSync(
      path.join(appData, "dev.seafood.blacktape", "youtube_cookies.txt"),
      "test-cookie",
      "utf8",
    );
    fs.writeFileSync(
      path.join(localAppData, "dev.seafood.blacktape", "yt-dlp.exe"),
      "test-ytdlp",
      "utf8",
    );
  });

  afterEach(() => {
    try {
      fs.rmSync(tmpBase, { recursive: true, force: true });
    } catch (_) {}
  });

  it("copies cookies and tools to fresh directory on Windows", () => {
    seedFreshToolsAndCookies("win32", {
      APPDATA: appData,
      LOCALAPPDATA: localAppData,
    });

    const dstCookies = path.join(
      appData,
      "dev.seafood.blacktape.fresh",
      "youtube_cookies.txt",
    );
    const dstYtdlp = path.join(
      localAppData,
      "dev.seafood.blacktape.fresh",
      "yt-dlp.exe",
    );

    expect(fs.existsSync(dstCookies)).toBe(true);
    expect(fs.readFileSync(dstCookies, "utf8")).toBe("test-cookie");
    expect(fs.existsSync(dstYtdlp)).toBe(true);
    expect(fs.readFileSync(dstYtdlp, "utf8")).toBe("test-ytdlp");
  });
});
