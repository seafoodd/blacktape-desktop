import type { ToolName, ToolStatus, ToolsStatus } from "@/shared/lib/audio.ts";

export const TOOL_NAMES: ToolName[] = ["ytDlp", "deno", "ffmpeg"];

export const TOOL_LABELS: Record<ToolName, string> = {
  ytDlp: "yt-dlp",
  deno: "Deno",
  ffmpeg: "FFmpeg",
};

/** What each tool is for, shown under its name in Settings. */
export const TOOL_PURPOSE: Record<ToolName, string> = {
  ytDlp: "Downloads from YouTube, Bandcamp and SoundCloud",
  deno: "Solves YouTube's JavaScript challenges",
  ffmpeg: "Converts downloads to audio files",
};

/** Bytes as megabytes with one decimal under 10 MB ("4.2 MB", "38 MB"). */
export function formatMb(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  const tenths = Math.round(mb * 10) / 10;
  return `${tenths < 10 ? tenths.toFixed(1) : Math.round(mb)} MB`;
}

/** Whether a tool is still being installed for the first time (jobs that need it wait). */
export function isSettingUp(tool: ToolStatus): boolean {
  return !tool.usable && tool.phase !== "failed" && tool.phase !== "unsupported";
}

export type BannerState =
  | { kind: "settingUp"; downloaded: number; total: number | null }
  | { kind: "failed"; error: string };

/**
 * The first-run banner: shown while any tool is installed for the first time, or when yt-dlp, without which
 * nothing downloads, could not be installed. `null` once everything is in place.
 */
export function bannerState(status: ToolsStatus | null): BannerState | null {
  if (!status) return null;
  const settingUp = TOOL_NAMES.map((name) => status[name]).filter(isSettingUp);
  if (settingUp.length > 0) {
    const downloaded = settingUp.reduce((sum, t) => sum + t.downloaded, 0);
    const totals = settingUp.map((t) => t.total);
    const total = totals.every((t) => t !== null)
      ? totals.reduce<number>((sum, t) => sum + (t ?? 0), 0)
      : null;
    return { kind: "settingUp", downloaded, total };
  }
  const ytDlp = status.ytDlp;
  if (!ytDlp.usable && ytDlp.phase === "failed") {
    return { kind: "failed", error: ytDlp.error ?? "unknown error" };
  }
  return null;
}

export function bannerText(state: BannerState): string {
  if (state.kind === "failed") {
    return `The downloader could not be set up: ${state.error}`;
  }
  if (state.downloaded === 0) return "Setting up the downloader…";
  const total = state.total !== null ? ` / ${formatMb(state.total)}` : "";
  return `Setting up the downloader… ${formatMb(state.downloaded)}${total}`;
}

/** One tool's state in words, for Settings. */
export function toolStatusText(tool: ToolStatus): string {
  switch (tool.phase) {
    case "checking":
      return tool.usable ? "Checking for updates…" : "Checking…";
    case "downloading": {
      const total = tool.total !== null ? ` / ${formatMb(tool.total)}` : "";
      return `Downloading… ${formatMb(tool.downloaded)}${total}`;
    }
    case "ready":
      return tool.source === "system" ? "Installed on this computer" : "Ready";
    case "failed":
      return `${tool.usable ? "Update failed" : "Setup failed"}: ${tool.error ?? "unknown error"}`;
    case "unsupported":
      return "Not available for this computer";
  }
}

/** Version, where it comes from and its size ("2026.10.26 · app copy · 22 MB"). */
export function toolDetails(tool: ToolStatus): string {
  const parts: string[] = [];
  if (tool.version) parts.push(tool.version);
  if (tool.source === "system") parts.push("system install");
  if (tool.source === "app") parts.push("app copy");
  if (tool.source === "app" && tool.size) parts.push(formatMb(tool.size));
  return parts.join(" · ");
}

/** Whether a check is running, so the buttons wait. */
export function anyBusy(status: ToolsStatus | null): boolean {
  return !!status && TOOL_NAMES.some((n) => ["checking", "downloading"].includes(status[n].phase));
}

export function anyFailed(status: ToolsStatus | null): boolean {
  return !!status && TOOL_NAMES.some((n) => status[n].phase === "failed");
}
