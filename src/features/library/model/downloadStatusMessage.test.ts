import { describe, expect, it } from "vitest";
import { torrentStatusMessage } from "./downloadStatusMessage.ts";

describe("torrentStatusMessage", () => {
  it("keeps torrent waiting and retry messages visible after progress", () => {
    expect(torrentStatusMessage("Waiting for torrent seeders (12s)...")).toBe("Waiting for torrent seeders (12s)...");
    expect(torrentStatusMessage("Retrying torrent via trackers...")).toBe("Retrying torrent via trackers...");
  });
  it("does not replace download percentages with ordinary track messages", () => {
    expect(torrentStatusMessage("Downloaded 1 of 10 tracks")).toBeNull();
  });
  it("shows when an Archive web seed is delivering the selected torrent", () => {
    const message = "Downloading from Archive web seed…";
    expect(torrentStatusMessage(message)).toBe(message);
  });
});
