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
