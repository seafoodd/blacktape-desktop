import { describe, expect, it } from "vitest";
import { shouldFollowLine } from "@/features/player/lib/lyricsFollow.ts";

describe("shouldFollowLine", () => {
  it("follows the active line while playing when the user is not scrolling", () => {
    expect(shouldFollowLine({ isPlaying: true, userScrolled: false, lineChanged: false })).toBe(true);
  });

  it("does not follow while paused when only the scroll pause ended", () => {
    expect(shouldFollowLine({ isPlaying: false, userScrolled: false, lineChanged: false })).toBe(false);
  });

  it("follows a changed line while paused, such as after a seek, when the user is not scrolling", () => {
    expect(shouldFollowLine({ isPlaying: false, userScrolled: false, lineChanged: true })).toBe(true);
  });

  it("never overrides a user who is scrolling", () => {
    for (const isPlaying of [true, false]) {
      for (const lineChanged of [true, false]) {
        expect(shouldFollowLine({ isPlaying, userScrolled: true, lineChanged })).toBe(false);
      }
    }
  });
});
