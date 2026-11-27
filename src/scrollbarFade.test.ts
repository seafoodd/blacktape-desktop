import { describe, it, expect } from "vitest";
import { createFader, SCROLLBAR_FADE_MS, type FadeClock } from "@/scrollbarFade.ts";

const D = SCROLLBAR_FADE_MS;
const last = (levels: number[]) => levels[levels.length - 1];

/** A clock whose time and frame requests are controlled by the test. */
function setup() {
  const state = { time: 0, frames: [] as Array<() => void> };
  const clock: FadeClock = {
    now: () => state.time,
    frame: (step) => {
      state.frames.push(step);
    },
  };
  const log: Array<[string, number]> = [];
  const fader = createFader<string>((target, level) => log.push([target, level]), clock);

  /** Runs the requested frame as if it happened at `at`. */
  const runFrame = (at: number) => {
    state.time = at;
    const step = state.frames.shift();
    if (!step) throw new Error("no frame was requested");
    step();
  };
  const show = (at: number, wanted: Set<string>) => {
    state.time = at;
    fader.show(wanted);
  };
  const levelsOf = (target: string) => log.filter(([t]) => t === target).map(([, level]) => level);
  return { fader, state, runFrame, show, log, levelsOf, pendingFrames: () => state.frames.length };
}

describe("SCROLLBAR_FADE_MS", () => {
  it("is the duration the fade runs for", () => {
    const { show, runFrame, levelsOf } = setup();
    show(0, new Set(["a"]));
    runFrame(D - 1);
    expect(levelsOf("a")[0]).toBeLessThan(1);
    runFrame(D);
    expect(last(levelsOf("a"))).toBe(1);
  });
});

describe("createFader", () => {
  it("fades a target in from 0 to 1 over SCROLLBAR_FADE_MS when shown", () => {
    const { show, runFrame, levelsOf, pendingFrames } = setup();
    show(0, new Set(["a"]));

    runFrame(D / 2);
    expect(levelsOf("a")).toEqual([0.5]);
    expect(pendingFrames()).toBe(1);

    runFrame(D);
    expect(levelsOf("a")).toEqual([0.5, 1]);
    expect(pendingFrames()).toBe(0);
  });

  it("fades a shown target out through 0.5 to 0 when the wanted set becomes empty", () => {
    const { show, runFrame, levelsOf, pendingFrames } = setup();
    show(0, new Set(["a"]));
    runFrame(D);

    show(D, new Set());
    runFrame(D + D / 2);
    expect(last(levelsOf("a"))).toBe(0.5);

    runFrame(D + D);
    expect(last(levelsOf("a"))).toBe(0);
    expect(pendingFrames()).toBe(0);
  });

  it("continues from the current level when turned around midway, without jumping to 1", () => {
    const { show, runFrame, levelsOf, pendingFrames } = setup();
    show(0, new Set(["a"]));
    runFrame(D / 2);
    expect(levelsOf("a")).toEqual([0.5]);

    // Hide while the level is 0.5: it continues from 0.5 and must not jump back up to 1.
    show(D / 2, new Set());
    runFrame(D); // half a duration after the turn: level 0.25
    expect(last(levelsOf("a"))).toBe(0.25);

    runFrame(D / 2 + D); // a full duration after the turn: level 0
    expect(last(levelsOf("a"))).toBe(0);
    expect(levelsOf("a")).toEqual([0.5, 0.25, 0]);
    expect(levelsOf("a")).not.toContain(1);
    expect(pendingFrames()).toBe(0);
  });

  it("fades two targets independently when the wanted set moves from a to b", () => {
    const { show, runFrame, levelsOf, pendingFrames } = setup();
    show(0, new Set(["a"]));
    runFrame(D);
    expect(levelsOf("a")).toEqual([1]);

    show(D, new Set(["b"]));
    runFrame(D + D / 2);
    expect(last(levelsOf("a"))).toBe(0.5); // a fades out
    expect(levelsOf("b")).toEqual([0.5]); // b fades in

    runFrame(D + D);
    expect(last(levelsOf("a"))).toBe(0);
    expect(last(levelsOf("b"))).toBe(1);
    expect(pendingFrames()).toBe(0);
  });

  it("does not request a frame or call apply again when shown with the same set while fully shown", () => {
    const { show, runFrame, log, pendingFrames } = setup();
    show(0, new Set(["a"]));
    runFrame(D);
    expect(pendingFrames()).toBe(0);
    log.length = 0;

    show(D + 10, new Set(["a"]));
    expect(pendingFrames()).toBe(0);
    expect(log).toEqual([]);
  });

  it("clamps to the final level when a frame arrives later than the duration", () => {
    const { show, runFrame, levelsOf, pendingFrames } = setup();
    show(0, new Set(["a"]));
    runFrame(D * 4);
    expect(levelsOf("a")).toEqual([1]);
    expect(pendingFrames()).toBe(0);

    show(D * 4, new Set());
    runFrame(D * 10);
    expect(last(levelsOf("a"))).toBe(0);
    expect(pendingFrames()).toBe(0);
  });
});
