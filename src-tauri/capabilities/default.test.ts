import { describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";

describe("Tauri default capabilities", () => {
  it("includes window permissions for focus, unminimize, and show", () => {
    const raw = fs.readFileSync(path.resolve(__dirname, "./default.json"), "utf8");
    const json = JSON.parse(raw);
    expect(json.permissions).toContain("core:window:allow-set-focus");
    expect(json.permissions).toContain("core:window:allow-unminimize");
    expect(json.permissions).toContain("core:window:allow-show");
  });
});

describe("torrent capability", () => {
  it("separates main-window discovery from Settings-only indexer management", () => {
    const capability = JSON.parse(fs.readFileSync(path.resolve(__dirname, "torrent.json"), "utf8"));
    const settings = JSON.parse(fs.readFileSync(path.resolve(__dirname, "torrent-settings.json"), "utf8"));
    const bridge = fs.readFileSync(path.resolve(__dirname, "../../src/shared/lib/audio.ts"), "utf8");
    const invokes = [...bridge.matchAll(/invoke\("plugin:torrent\|([a-z_]+)"/g)]
      .map(([, command]) => `torrent:allow-${command.replaceAll("_", "-")}`);
    expect(capability.windows).toEqual(["main"]);
    expect(capability.permissions).toEqual([
      "torrent:allow-search-torrent-releases",
      "torrent:allow-inspect-torrent-release",
      "torrent:allow-resolve-torrent-album",
      "torrent:allow-cancel-torrent-inspection",
      "torrent:allow-is-rutracker-enabled",
    ]);
    expect(settings.windows).toEqual(["settings"]);
    expect(settings.permissions).toEqual([
      "torrent:allow-list-torrent-indexers",
      "torrent:allow-set-torrent-indexer-enabled",
      "torrent:allow-test-torrent-indexer",
      "torrent:allow-open-rutracker-login",
      "torrent:allow-get-rutracker-status",
      "torrent:allow-sign-in-rutracker",
      "torrent:allow-sign-out-rutracker",
    ]);
    expect([...capability.permissions, ...settings.permissions].sort()).toEqual(invokes.sort());
  });
});
