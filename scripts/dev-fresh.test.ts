import { describe, expect, it, vi } from "vitest";
import { getFreshDirs, cleanFreshDirs } from "./dev-fresh.js";
import fs from "node:fs";

describe("getFreshDirs", () => {
  it("returns Windows AppData and LocalAppData paths", () => {
    const dirs = getFreshDirs("win32", {
      APPDATA: "C:\\Users\\Test\\AppData\\Roaming",
      LOCALAPPDATA: "C:\\Users\\Test\\AppData\\Local",
    });
    expect(dirs).toContain("C:\\Users\\Test\\AppData\\Roaming\\dev.seafood.blacktape.fresh");
    expect(dirs).toContain("C:\\Users\\Test\\AppData\\Local\\dev.seafood.blacktape.fresh");
  });

  it("returns macOS Application Support path", () => {
    const dirs = getFreshDirs("darwin", {}, "/Users/test");
    expect(dirs).toEqual(["/Users/test/Library/Application Support/dev.seafood.blacktape.fresh"]);
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
