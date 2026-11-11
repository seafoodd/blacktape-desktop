import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  CHECK_INTERVAL_CHANNEL,
  CHECK_INTERVAL_KEY,
  CHECK_INTERVAL_OPTIONS,
  DEFAULT_CHECK_INTERVAL_HOURS,
  checkIntervalMs,
  parseCheckInterval,
  readCheckInterval,
  writeCheckInterval,
} from "./checkInterval";

describe("parseCheckInterval", () => {
  it.each([null, undefined, "", "  "])("falls back to the default for %j", (raw) => {
    expect(parseCheckInterval(raw)).toBe(DEFAULT_CHECK_INTERVAL_HOURS);
  });

  it.each([
    ["1", 1],
    ["3", 3],
    ["6", 6],
    ["12", 12],
    ["24", 24],
    ["0", 0],
  ])("returns the stored option %j as %i", (raw, expected) => {
    expect(parseCheckInterval(raw)).toBe(expected);
  });

  it.each(["5", "abc", "-1", "1.5"])("treats %j (not an option) as the default", (raw) => {
    expect(parseCheckInterval(raw)).toBe(1);
  });
});

describe("checkIntervalMs", () => {
  it("converts hours to milliseconds", () => {
    expect(checkIntervalMs(1)).toBe(3_600_000);
    expect(checkIntervalMs(24)).toBe(86_400_000);
  });

  it("returns null when there is no periodic check", () => {
    expect(checkIntervalMs(0)).toBeNull();
    expect(checkIntervalMs(-1)).toBeNull();
  });
});

describe("CHECK_INTERVAL_OPTIONS", () => {
  it("has unique hours", () => {
    const hours = CHECK_INTERVAL_OPTIONS.map((o) => o.hours);
    expect(new Set(hours).size).toBe(hours.length);
  });

  it("contains 0 and the default", () => {
    const hours = CHECK_INTERVAL_OPTIONS.map((o) => o.hours);
    expect(hours).toContain(0);
    expect(hours).toContain(DEFAULT_CHECK_INTERVAL_HOURS);
  });
});

describe("readCheckInterval / writeCheckInterval", () => {
  let store: Map<string, string>;
  let storage: { getItem: ReturnType<typeof vi.fn>; setItem: ReturnType<typeof vi.fn> };
  let posted: { name: string; message: unknown }[];
  let closeCount: number;

  beforeEach(() => {
    store = new Map();
    storage = {
      getItem: vi.fn((key: string) => store.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => {
        store.set(key, String(value));
      }),
    };
    vi.stubGlobal("localStorage", storage);

    posted = [];
    closeCount = 0;
    class FakeBroadcastChannel {
      constructor(public readonly name: string) {}
      postMessage(message: unknown) {
        posted.push({ name: this.name, message });
      }
      close() {
        closeCount += 1;
      }
    }
    vi.stubGlobal("BroadcastChannel", FakeBroadcastChannel);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("reads back what was written", () => {
    writeCheckInterval(6);
    expect(storage.setItem).toHaveBeenCalledWith(CHECK_INTERVAL_KEY, "6");
    expect(readCheckInterval()).toBe(6);
  });

  it("posts a change message and closes the channel", () => {
    writeCheckInterval(6);
    expect(posted).toEqual([
      { name: CHECK_INTERVAL_CHANNEL, message: { type: "CHECK_INTERVAL_CHANGED", hours: 6 } },
    ]);
    expect(closeCount).toBe(1);
  });

  it("reads the default when the stored value is garbage", () => {
    store.set(CHECK_INTERVAL_KEY, "garbage");
    expect(readCheckInterval()).toBe(1);
  });

  it("reads the default when localStorage.getItem throws", () => {
    storage.getItem.mockImplementation(() => {
      throw new Error("storage denied");
    });
    expect(readCheckInterval()).toBe(DEFAULT_CHECK_INTERVAL_HOURS);
  });

  it("still posts the change message when setItem throws", () => {
    storage.setItem.mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    expect(() => writeCheckInterval(3)).not.toThrow();
    expect(posted).toEqual([
      { name: CHECK_INTERVAL_CHANNEL, message: { type: "CHECK_INTERVAL_CHANGED", hours: 3 } },
    ]);
  });
});
