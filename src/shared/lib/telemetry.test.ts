import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { captureFrontendError, isCancellationError } from "./telemetry.ts";

describe("isCancellationError", () => {
  it("identifies auth_window_closed as cancellation", () => {
    expect(isCancellationError("auth_window_closed")).toBe(true);
    expect(isCancellationError(new Error("auth_window_closed"))).toBe(true);
    expect(
      isCancellationError(
        new Error("[libraryStore.startDownload] auth_window_closed"),
      ),
    ).toBe(true);
    expect(isCancellationError("auth window closed")).toBe(true);
    expect(isCancellationError("user_cancelled")).toBe(true);
  });

  it("returns false for regular errors and empty values", () => {
    expect(isCancellationError("network timeout")).toBe(false);
    expect(isCancellationError(new Error("Database connection failed"))).toBe(false);
    expect(isCancellationError(null)).toBe(false);
    expect(isCancellationError(undefined)).toBe(false);
  });
});

describe("captureFrontendError", () => {
  beforeEach(() => {
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    delete (globalThis as unknown as { window?: unknown }).window;
    vi.restoreAllMocks();
  });

  it("logs error to console without rethrowing if window is undefined", () => {
    captureFrontendError(new Error("test error"), "testContext");

    expect(console.error).toHaveBeenCalledWith(
      "[testContext]",
      expect.any(Error),
    );
  });

  it("logs error to console without rethrowing if Sentry bridge is not present", () => {
    (globalThis as unknown as { window: unknown }).window = {};

    captureFrontendError(new Error("test error"), "testContext");

    expect(console.error).toHaveBeenCalledWith(
      "[testContext]",
      expect.any(Error),
    );
  });

  it("rethrows error in microtask when Sentry bridge is active", async () => {
    (globalThis as unknown as { window: unknown }).window = {
      __SENTRY__: {},
    };

    let caughtError: unknown = null;
    const originalQueueMicrotask = globalThis.queueMicrotask;
    globalThis.queueMicrotask = (fn: () => void) => {
      try {
        fn();
      } catch (err) {
        caughtError = err;
      }
    };

    try {
      captureFrontendError("network timeout", "api");
      expect(caughtError).toBeInstanceOf(Error);
      expect((caughtError as Error).message).toBe("[api] network timeout");
    } finally {
      globalThis.queueMicrotask = originalQueueMicrotask;
    }
  });

  it("does not rethrow in microtask when error is an auth_window_closed cancellation", () => {
    (globalThis as unknown as { window: unknown }).window = {
      __SENTRY__: {},
    };

    let scheduledMicrotask = false;
    const originalQueueMicrotask = globalThis.queueMicrotask;
    globalThis.queueMicrotask = () => {
      scheduledMicrotask = true;
    };

    try {
      captureFrontendError("auth_window_closed", "libraryStore.startDownload");
      expect(scheduledMicrotask).toBe(false);

      captureFrontendError(new Error("auth_window_closed"), "auth");
      expect(scheduledMicrotask).toBe(false);
    } finally {
      globalThis.queueMicrotask = originalQueueMicrotask;
    }
  });
});
