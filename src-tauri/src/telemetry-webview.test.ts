import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const source = readFileSync(new URL("./telemetry-webview.js", import.meta.url), "utf8");

function initializeTelemetry(url: string, devOrigin: string | null = null) {
  const startSentry = vi.fn();
  runInNewContext(`(${source})(devOrigin, startSentry)`, {
    window: { location: new URL(url) },
    devOrigin,
    startSentry,
  });
  return startSentry;
}

describe("Sentry webview initialization", () => {
  it.each(["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"])(
    "starts on the packaged app at %s",
    (url) => expect(initializeTelemetry(url)).toHaveBeenCalledOnce(),
  );

  it("starts on the configured development origin", () => {
    expect(initializeTelemetry("http://localhost:1420/settings", "http://localhost:1420"))
      .toHaveBeenCalledOnce();
  });

  it.each([
    ["https://rutracker.org/forum/login.php", "http://localhost:1420"],
    ["https://accounts.google.com/", "http://localhost:1420"],
    ["https://tauri.localhost.example.org/", null],
    ["http://localhost:1420/", null],
    ["http://localhost:1421/", "http://localhost:1420"],
    ["about:blank", null],
  ])("skips %s with development origin %s", (url, devOrigin) => {
    expect(initializeTelemetry(url, devOrigin)).not.toHaveBeenCalled();
  });
});
