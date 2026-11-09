import { describe, expect, it } from "vitest";
import { downloadStatusLabel } from "./downloadStatusLabel.ts";

describe("downloadStatusLabel", () => {
  it("labels downloaded and partly downloaded items", () => {
    expect(downloadStatusLabel("downloaded")).toBe("Downloaded");
    expect(downloadStatusLabel("partial")).toBe("Partly downloaded");
  });

  it("has no label for none (nothing is rendered)", () => {
    expect(downloadStatusLabel("none")).toBeNull();
  });
});
