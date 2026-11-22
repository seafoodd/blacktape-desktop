import { describe, it, expect } from "vitest";
import {
  TELEMETRY_COPY,
  telemetryToggleLabel,
} from "./telemetrySettingsHelper.ts";

describe("telemetrySettingsHelper", () => {
  it("provides clear privacy description copy", () => {
    expect(TELEMETRY_COPY.title).toBe("Privacy & Telemetry");
    expect(TELEMETRY_COPY.description).toContain("anonymous crash reports");
    expect(TELEMETRY_COPY.description).toContain("No personal data");
  });

  it("returns appropriate toggle status label", () => {
    expect(telemetryToggleLabel(true)).toBe("Send anonymous crash reports & diagnostics");
    expect(telemetryToggleLabel(false)).toBe("Send anonymous crash reports & diagnostics (disabled)");
  });
});
