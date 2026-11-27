import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SEARCH_PROGRESS_STYLE_KEY,
  SEARCH_PROGRESS_STYLES,
  parseSearchProgressStyle,
  readSearchProgressStyle,
  subscribeSearchProgressStyle,
  writeSearchProgressStyle,
} from "@/features/search/model/searchProgressStyle.ts";

type StorageListener = (event: StorageEvent) => void;

/** A minimal window with addEventListener/removeEventListener and a way to fire a fake "storage" event. */
function stubWindow() {
  const storageListeners = new Set<StorageListener>();
  const addEventListener = vi.fn((type: string, listener: StorageListener) => {
    if (type === "storage") storageListeners.add(listener);
  });
  const removeEventListener = vi.fn((type: string, listener: StorageListener) => {
    if (type === "storage") storageListeners.delete(listener);
  });
  vi.stubGlobal("window", { addEventListener, removeEventListener });

  const dispatchStorage = (key: string | null) => {
    storageListeners.forEach((listener) => listener({ key } as StorageEvent));
  };

  return { addEventListener, removeEventListener, dispatchStorage, listenerCount: () => storageListeners.size };
}

function stubLocalStorage() {
  const storage = {
    getItem: vi.fn<(key: string) => string | null>(() => null),
    setItem: vi.fn<(key: string, value: string) => void>(),
  };
  vi.stubGlobal("localStorage", storage);
  return storage;
}

const unsubscribers: Array<() => void> = [];

function subscribe(onChange: () => void): () => void {
  const unsubscribe = subscribeSearchProgressStyle(onChange);
  unsubscribers.push(unsubscribe);
  return unsubscribe;
}

afterEach(() => {
  unsubscribers.splice(0).forEach((unsubscribe) => unsubscribe());
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("parseSearchProgressStyle", () => {
  it("keeps the known styles", () => {
    expect(parseSearchProgressStyle("pulse")).toBe("pulse");
    expect(parseSearchProgressStyle("dashboard")).toBe("dashboard");
  });

  it.each([
    ["null", null],
    ["undefined", undefined],
    ["an empty string", ""],
    ["garbage", "garbage"],
  ])("falls back to pulse for %s", (_label, raw) => {
    expect(parseSearchProgressStyle(raw)).toBe("pulse");
  });
});

describe("readSearchProgressStyle", () => {
  beforeEach(() => {
    stubWindow();
  });

  it("returns the stored style", () => {
    const storage = stubLocalStorage();
    storage.getItem.mockReturnValue("dashboard");

    expect(readSearchProgressStyle()).toBe("dashboard");
    expect(storage.getItem).toHaveBeenCalledWith(SEARCH_PROGRESS_STYLE_KEY);
  });

  it("returns pulse when nothing is stored", () => {
    stubLocalStorage();

    expect(readSearchProgressStyle()).toBe("pulse");
  });

  it("returns pulse when localStorage.getItem throws", () => {
    const storage = stubLocalStorage();
    storage.getItem.mockImplementation(() => {
      throw new Error("storage is disabled");
    });

    expect(readSearchProgressStyle()).toBe("pulse");
  });
});

describe("writeSearchProgressStyle", () => {
  beforeEach(() => {
    stubWindow();
  });

  it("stores the style under SEARCH_PROGRESS_STYLE_KEY", () => {
    const storage = stubLocalStorage();

    writeSearchProgressStyle("dashboard");

    expect(storage.setItem).toHaveBeenCalledWith(SEARCH_PROGRESS_STYLE_KEY, "dashboard");
  });

  it("notifies the subscribers of this window", () => {
    stubLocalStorage();
    const onChange = vi.fn();
    subscribe(onChange);

    writeSearchProgressStyle("dashboard");

    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("does not throw when setItem throws, and logs the error", () => {
    const storage = stubLocalStorage();
    storage.setItem.mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    expect(() => writeSearchProgressStyle("dashboard")).not.toThrow();
    expect(errorSpy).toHaveBeenCalledTimes(1);
  });
});

describe("subscribeSearchProgressStyle", () => {
  it("calls back on a storage event for the style key", () => {
    const windowStub = stubWindow();
    stubLocalStorage();
    const onChange = vi.fn();
    subscribe(onChange);

    windowStub.dispatchStorage(SEARCH_PROGRESS_STYLE_KEY);

    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("calls back on a storage event with a null key (storage cleared)", () => {
    const windowStub = stubWindow();
    stubLocalStorage();
    const onChange = vi.fn();
    subscribe(onChange);

    windowStub.dispatchStorage(null);

    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("does not call back on a storage event for another key", () => {
    const windowStub = stubWindow();
    stubLocalStorage();
    const onChange = vi.fn();
    subscribe(onChange);

    windowStub.dispatchStorage("some_other_key");

    expect(onChange).not.toHaveBeenCalled();
  });

  it("stops calling back after unsubscribing and removes the window listener", () => {
    const windowStub = stubWindow();
    stubLocalStorage();
    const onChange = vi.fn();
    const unsubscribe = subscribeSearchProgressStyle(onChange);

    unsubscribe();
    writeSearchProgressStyle("dashboard");
    windowStub.dispatchStorage(SEARCH_PROGRESS_STYLE_KEY);

    expect(onChange).not.toHaveBeenCalled();
    expect(windowStub.removeEventListener).toHaveBeenCalledTimes(1);
    expect(windowStub.removeEventListener.mock.calls[0][0]).toBe("storage");
    expect(windowStub.removeEventListener.mock.calls[0][1]).toBe(
      windowStub.addEventListener.mock.calls[0][1],
    );
    expect(windowStub.listenerCount()).toBe(0);
  });
});

describe("SEARCH_PROGRESS_STYLES", () => {
  it("has unique ids and pulse first", () => {
    const ids = SEARCH_PROGRESS_STYLES.map((style) => style.id);

    expect(new Set(ids).size).toBe(ids.length);
    expect(ids[0]).toBe("pulse");
  });
});
