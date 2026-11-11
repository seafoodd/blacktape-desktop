import { describe, expect, it } from "vitest";
import type { ToolStatus, ToolsStatus } from "@/shared/lib/audio.ts";
import {
  anyBusy,
  anyFailed,
  bannerState,
  bannerText,
  formatMb,
  isSettingUp,
  toolDetails,
  toolStatusText,
} from "./toolsStatus.ts";

const MB = 1024 * 1024;

function tool(overrides: Partial<ToolStatus>): ToolStatus {
  return {
    phase: "ready",
    usable: true,
    version: null,
    source: null,
    size: null,
    downloaded: 0,
    total: null,
    error: null,
    ...overrides,
  };
}

function all(overrides: Partial<ToolsStatus>): ToolsStatus {
  return {
    seq: 0,
    ytDlp: tool({}),
    deno: tool({}),
    ffmpeg: tool({}),
    ...overrides,
  };
}

describe("formatMb", () => {
  it("shows a value that rounds up to 10 MB without a decimal", () => {
    expect(formatMb(9.97 * 1024 * 1024)).toBe("10 MB");
  });

  it("keeps one decimal under 10 MB", () => {
    expect(formatMb(4.2 * MB)).toBe("4.2 MB");
  });

  it("rounds to a whole number from 10 MB up", () => {
    expect(formatMb(38 * MB)).toBe("38 MB");
  });

  it("formats zero with one decimal", () => {
    expect(formatMb(0)).toBe("0.0 MB");
  });
});

describe("isSettingUp", () => {
  it("is false for a usable, ready tool", () => {
    expect(isSettingUp(tool({ phase: "ready", usable: true }))).toBe(false);
  });

  it("is false for a failed tool that is not usable", () => {
    expect(isSettingUp(tool({ phase: "failed", usable: false }))).toBe(false);
  });

  it("is false for an unsupported tool", () => {
    expect(isSettingUp(tool({ phase: "unsupported", usable: false }))).toBe(false);
  });

  it("is true while checking a tool that is not usable yet", () => {
    expect(isSettingUp(tool({ phase: "checking", usable: false }))).toBe(true);
  });

  it("is true while downloading a tool that is not usable yet", () => {
    expect(isSettingUp(tool({ phase: "downloading", usable: false }))).toBe(true);
  });

  it("is false while downloading an update to a tool that is already usable", () => {
    expect(isSettingUp(tool({ phase: "downloading", usable: true }))).toBe(false);
  });
});

describe("bannerState", () => {
  it("stays hidden while tools are only being looked up at start", () => {
    const status = all({
      deno: tool({ phase: "checking", usable: false }),
      ffmpeg: tool({ phase: "checking", usable: false }),
    });
    expect(bannerState(status)).toBeNull();
  });

  it("is null without a status", () => {
    expect(bannerState(null)).toBeNull();
  });

  it("is null when everything is ready", () => {
    expect(bannerState(all({}))).toBeNull();
  });

  it("sums the downloaded bytes and totals of tools being set up", () => {
    const status = all({
      ytDlp: tool({ phase: "downloading", usable: false, downloaded: 2 * MB, total: 3 * MB }),
      deno: tool({ phase: "downloading", usable: false, downloaded: 1 * MB, total: 5 * MB }),
    });
    expect(bannerState(status)).toEqual({
      kind: "settingUp",
      downloaded: 3 * MB,
      total: 8 * MB,
    });
  });

  it("has a null total when one tool being set up has no total", () => {
    const status = all({
      ytDlp: tool({ phase: "downloading", usable: false, downloaded: 2 * MB, total: null }),
      deno: tool({ phase: "downloading", usable: false, downloaded: 1 * MB, total: 5 * MB }),
    });
    expect(bannerState(status)).toEqual({
      kind: "settingUp",
      downloaded: 3 * MB,
      total: null,
    });
  });

  it("does not show the banner for an update to a usable tool", () => {
    const status = all({
      ytDlp: tool({ phase: "downloading", usable: true, downloaded: 2 * MB, total: 3 * MB }),
    });
    expect(bannerState(status)).toBeNull();
  });

  it("shows a failed banner when yt-dlp is failed and not usable", () => {
    const status = all({
      ytDlp: tool({ phase: "failed", usable: false, error: "network down" }),
    });
    expect(bannerState(status)).toEqual({ kind: "failed", error: "network down" });
  });

  it("shows no banner when only deno failed", () => {
    const status = all({
      deno: tool({ phase: "failed", usable: false, error: "network down" }),
    });
    expect(bannerState(status)).toBeNull();
  });
});

describe("bannerText", () => {
  it("says the downloader is being set up when nothing is downloaded yet", () => {
    expect(bannerText({ kind: "settingUp", downloaded: 0, total: null })).toBe(
      "Setting up the downloader…",
    );
  });

  it("shows the downloaded and total sizes joined by a slash", () => {
    expect(bannerText({ kind: "settingUp", downloaded: 4.2 * MB, total: 38 * MB })).toBe(
      "Setting up the downloader… 4.2 MB / 38 MB",
    );
  });

  it("shows only the downloaded size when the total is unknown", () => {
    expect(bannerText({ kind: "settingUp", downloaded: 4.2 * MB, total: null })).toBe(
      "Setting up the downloader… 4.2 MB",
    );
  });

  it("includes the error when setup failed", () => {
    expect(bannerText({ kind: "failed", error: "network down" })).toContain("network down");
  });
});

describe("toolStatusText", () => {
  it("says it is checking for updates when the tool is usable", () => {
    expect(toolStatusText(tool({ phase: "checking", usable: true }))).toBe(
      "Checking for updates…",
    );
  });

  it("says it is checking when the tool is not usable yet", () => {
    expect(toolStatusText(tool({ phase: "checking", usable: false }))).toBe("Checking…");
  });

  it("shows progress with the total while downloading", () => {
    expect(
      toolStatusText(tool({ phase: "downloading", usable: false, downloaded: 4.2 * MB, total: 38 * MB })),
    ).toBe("Downloading… 4.2 MB / 38 MB");
  });

  it("shows progress without a total when the total is unknown", () => {
    expect(
      toolStatusText(tool({ phase: "downloading", usable: false, downloaded: 4.2 * MB, total: null })),
    ).toBe("Downloading… 4.2 MB");
  });

  it("says a system install is ready", () => {
    expect(toolStatusText(tool({ phase: "ready", source: "system" }))).toBe(
      "Ready",
    );
  });

  it("says the downloaded copy is ready", () => {
    expect(toolStatusText(tool({ phase: "ready", source: "app" }))).toBe("Ready");
  });

  it("says an update failed when a usable tool fails", () => {
    expect(toolStatusText(tool({ phase: "failed", usable: true, error: "boom" }))).toBe(
      "Update failed: boom",
    );
  });

  it("says setup failed when a tool that is not usable fails", () => {
    expect(toolStatusText(tool({ phase: "failed", usable: false, error: "boom" }))).toBe(
      "Setup failed: boom",
    );
  });

  it("falls back to an unknown error when a failed tool has no error text", () => {
    expect(toolStatusText(tool({ phase: "failed", usable: false, error: null }))).toBe(
      "Setup failed: unknown error",
    );
  });

  it("says the tool is not available for this computer when unsupported", () => {
    expect(toolStatusText(tool({ phase: "unsupported", usable: false }))).toBe(
      "Not available for this computer",
    );
  });
});

describe("toolDetails", () => {
  it("joins version, who installed it and size", () => {
    expect(
      toolDetails(tool({ version: "2026.10.26", source: "app", size: 22 * MB })),
    ).toBe("2026.10.26 · downloaded by Blacktape · 22 MB");
  });

  it("shows a system install without a size", () => {
    expect(
      toolDetails(tool({ version: "2026.10.26", source: "system", size: 22 * MB })),
    ).toBe("2026.10.26 · installed on your system");
  });

  it("is empty when nothing is known about the tool", () => {
    expect(toolDetails(tool({}))).toBe("");
  });
});

describe("anyBusy", () => {
  it("is false without a status", () => {
    expect(anyBusy(null)).toBe(false);
  });

  it("is false when every tool is ready", () => {
    expect(anyBusy(all({}))).toBe(false);
  });

  it("is true while a tool is checking", () => {
    expect(anyBusy(all({ deno: tool({ phase: "checking", usable: true }) }))).toBe(true);
  });

  it("is true while a tool is downloading", () => {
    expect(
      anyBusy(all({ ffmpeg: tool({ phase: "downloading", usable: false }) })),
    ).toBe(true);
  });

  it("is false when a tool has failed and nothing is in progress", () => {
    expect(anyBusy(all({ ytDlp: tool({ phase: "failed", usable: false }) }))).toBe(false);
  });
});

describe("anyFailed", () => {
  it("is false without a status", () => {
    expect(anyFailed(null)).toBe(false);
  });

  it("is false when every tool is ready", () => {
    expect(anyFailed(all({}))).toBe(false);
  });

  it("is true when any tool has failed", () => {
    expect(anyFailed(all({ deno: tool({ phase: "failed", usable: true, error: "x" }) }))).toBe(
      true,
    );
  });

  it("is false for an unsupported tool", () => {
    expect(anyFailed(all({ ffmpeg: tool({ phase: "unsupported", usable: false }) }))).toBe(false);
  });
});
