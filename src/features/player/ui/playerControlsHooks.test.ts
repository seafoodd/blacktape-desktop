import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

describe("PlayerControls hook rules", () => {
  it("does not call hooks conditionally with ternary or logical operators", () => {
    const fileContent = fs.readFileSync(
      path.resolve(__dirname, "PlayerControls.tsx"),
      "utf-8",
    );
    // React hooks must never be called conditionally inside a ternary or boolean expression
    const conditionalHookMatch = fileContent.match(/(\?|&&|\|\|)\s*use[A-Z]\w*\(/g);
    expect(conditionalHookMatch).toBeNull();
  });
});
