import { beforeEach, describe, expect, it, vi } from "vitest";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  downloadPercent,
  installLabel,
  shouldFlash,
  useUpdateStore,
} from "./updateStore.ts";
import { flashTaskbar } from "../lib/flashTaskbar";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("../lib/flashTaskbar", () => ({ flashTaskbar: vi.fn(async () => {}) }));

const checkMock = vi.mocked(check);
const relaunchMock = vi.mocked(relaunch);
const flashTaskbarMock = vi.mocked(flashTaskbar);

const makeUpdate = (downloadAndInstall = vi.fn(async () => {})) =>
  ({ version: "0.5.0", body: "  notes  ", downloadAndInstall }) as any;

beforeEach(() => {
  checkMock.mockReset();
  relaunchMock.mockReset();
  flashTaskbarMock.mockReset();
  flashTaskbarMock.mockImplementation(async () => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
  useUpdateStore.setState({
    status: "idle",
    version: null,
    notes: null,
    downloaded: 0,
    total: null,
    error: null,
    update: null,
  });
});

describe("downloadPercent", () => {
  it("is null while the total is unknown, zero or negative", () => {
    expect(downloadPercent(10, null)).toBeNull();
    expect(downloadPercent(10, 0)).toBeNull();
    expect(downloadPercent(10, -5)).toBeNull();
  });

  it("returns whole percent for known totals", () => {
    expect(downloadPercent(0, 100)).toBe(0);
    expect(downloadPercent(50, 100)).toBe(50);
    expect(downloadPercent(100, 100)).toBe(100);
  });

  it("floors fractional percent", () => {
    expect(downloadPercent(1, 3)).toBe(33);
    expect(downloadPercent(2, 3)).toBe(66);
  });

  it("clamps to the 0 to 100 range", () => {
    expect(downloadPercent(150, 100)).toBe(100);
    expect(downloadPercent(-10, 100)).toBe(0);
  });
});

describe("installLabel", () => {
  it("shows an indeterminate downloading label when the total is unknown", () => {
    expect(installLabel("downloading", "0.5.0", 42, null)).toBe("Downloading…");
  });

  it("shows the download percent when known", () => {
    expect(installLabel("downloading", "0.5.0", 42, 100)).toBe("Downloading 42%");
  });

  it("shows the installing label", () => {
    expect(installLabel("installing", "0.5.0", 100, 100)).toBe("Installing…");
  });

  it("names the version when an update is available", () => {
    expect(installLabel("available", "0.5.0", 0, null)).toBe("Install v0.5.0 and restart");
  });

  it("falls back to a generic label without a version", () => {
    expect(installLabel("available", null, 0, null)).toBe("Install and restart");
  });
});

describe("checkForUpdate", () => {
  it("sets upToDate when the check finds nothing", async () => {
    checkMock.mockResolvedValue(null);
    await useUpdateStore.getState().checkForUpdate();
    const state = useUpdateStore.getState();
    expect(state.status).toBe("upToDate");
    expect(state.update).toBeNull();
  });

  it("stores the found update with its version and trimmed notes", async () => {
    const update = makeUpdate();
    checkMock.mockResolvedValue(update);
    await useUpdateStore.getState().checkForUpdate();
    const state = useUpdateStore.getState();
    expect(state.status).toBe("available");
    expect(state.update).toBe(update);
    expect(state.version).toBe("0.5.0");
    expect(state.notes).toBe("notes");
  });

  it("stores null notes when the body is blank", async () => {
    checkMock.mockResolvedValue({ ...makeUpdate(), body: "   " } as any);
    await useUpdateStore.getState().checkForUpdate();
    expect(useUpdateStore.getState().notes).toBeNull();
  });

  it("stores null notes when the body is missing", async () => {
    checkMock.mockResolvedValue({ ...makeUpdate(), body: undefined } as any);
    await useUpdateStore.getState().checkForUpdate();
    expect(useUpdateStore.getState().notes).toBeNull();
  });

  it("sets error status with the message on a non-silent failure", async () => {
    checkMock.mockRejectedValue(new Error("network down"));
    await useUpdateStore.getState().checkForUpdate();
    const state = useUpdateStore.getState();
    expect(state.status).toBe("error");
    expect(state.error).toContain("network down");
  });

  it("returns to idle without an error on a silent failure", async () => {
    checkMock.mockRejectedValue({ silent: true });
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    const state = useUpdateStore.getState();
    expect(state.status).toBe("idle");
    expect(state.error).toBeNull();
  });

  it("keeps an available update when a silent check fails", async () => {
    const update = makeUpdate();
    useUpdateStore.setState({ status: "available", update, version: "0.5.0" });
    checkMock.mockRejectedValue(new Error("offline"));
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    const state = useUpdateStore.getState();
    expect(state.status).toBe("available");
    expect(state.update).toBe(update);
    expect(state.error).toBeNull();
  });

  it.each(["checking", "downloading", "installing"] as const)(
    "does not check while status is %s",
    async (status) => {
      useUpdateStore.setState({ status });
      await useUpdateStore.getState().checkForUpdate();
      expect(checkMock).not.toHaveBeenCalled();
      expect(useUpdateStore.getState().status).toBe(status);
    },
  );
});

describe("installUpdate", () => {
  it("does nothing without an update", async () => {
    await useUpdateStore.getState().installUpdate();
    expect(relaunchMock).not.toHaveBeenCalled();
    expect(useUpdateStore.getState().status).toBe("idle");
  });

  it("tracks progress through downloading and installing, then relaunches", async () => {
    const seen: string[] = [];
    const downloadAndInstall = vi.fn(async (cb: (event: any) => void) => {
      cb({ event: "Started", data: { contentLength: 200 } });
      seen.push(useUpdateStore.getState().status);
      cb({ event: "Progress", data: { chunkLength: 50 } });
      cb({ event: "Progress", data: { chunkLength: 50 } });
      cb({ event: "Finished" });
      seen.push(useUpdateStore.getState().status);
    });
    useUpdateStore.setState({ status: "available", update: makeUpdate(downloadAndInstall) });

    await useUpdateStore.getState().installUpdate();

    const state = useUpdateStore.getState();
    expect(seen).toEqual(["downloading", "installing"]);
    expect(state.downloaded).toBe(100);
    expect(state.total).toBe(200);
    expect(relaunchMock).toHaveBeenCalledTimes(1);
  });

  it("leaves the total null when the download start has no content length", async () => {
    const downloadAndInstall = vi.fn(async (cb: (event: any) => void) => {
      cb({ event: "Started", data: {} });
      cb({ event: "Finished" });
    });
    useUpdateStore.setState({ status: "available", update: makeUpdate(downloadAndInstall) });

    await useUpdateStore.getState().installUpdate();
    expect(useUpdateStore.getState().total).toBeNull();
  });

  it("sets error and does not relaunch when the install fails", async () => {
    const downloadAndInstall = vi.fn(async () => {
      throw new Error("signature mismatch");
    });
    useUpdateStore.setState({ status: "available", update: makeUpdate(downloadAndInstall) });

    await useUpdateStore.getState().installUpdate();
    const state = useUpdateStore.getState();
    expect(state.status).toBe("error");
    expect(state.error).toContain("signature mismatch");
    expect(relaunchMock).not.toHaveBeenCalled();
  });

  it("ignores a second install while already downloading", async () => {
    const downloadAndInstall = vi.fn(async () => {});
    useUpdateStore.setState({
      status: "downloading",
      update: makeUpdate(downloadAndInstall),
    });

    await useUpdateStore.getState().installUpdate();
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunchMock).not.toHaveBeenCalled();
    expect(useUpdateStore.getState().status).toBe("downloading");
  });
});

describe("shouldFlash", () => {
  it("is true for a background check that finds a new version", () => {
    expect(shouldFlash(true, "0.4.9", "0.5.0")).toBe(true);
  });

  it("is false for a check the user asked for", () => {
    expect(shouldFlash(false, null, "0.5.0")).toBe(false);
  });

  it("is false for a background check that finds the already known version", () => {
    expect(shouldFlash(true, "0.5.0", "0.5.0")).toBe(false);
  });

  it("is true for a background check when no version was known yet", () => {
    expect(shouldFlash(true, null, "0.5.0")).toBe(true);
  });
});

describe("checkForUpdate taskbar flash", () => {
  it("flashes once when a silent check finds a version from idle", async () => {
    checkMock.mockResolvedValue(makeUpdate());
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    expect(flashTaskbarMock).toHaveBeenCalledTimes(1);
  });

  it("does not flash again when a second silent check finds the same version", async () => {
    checkMock.mockResolvedValue(makeUpdate());
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    expect(flashTaskbarMock).toHaveBeenCalledTimes(1);
  });

  it("does not flash for a check the user asked for", async () => {
    checkMock.mockResolvedValue(makeUpdate());
    await useUpdateStore.getState().checkForUpdate();
    expect(flashTaskbarMock).not.toHaveBeenCalled();
  });

  it("flashes when a silent check finds 0.5.0 while 0.4.9 was known", async () => {
    useUpdateStore.setState({ version: "0.4.9" });
    checkMock.mockResolvedValue(makeUpdate());
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    expect(flashTaskbarMock).toHaveBeenCalledTimes(1);
  });

  it("does not flash when the check finds no update", async () => {
    checkMock.mockResolvedValue(null);
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    expect(flashTaskbarMock).not.toHaveBeenCalled();
  });

  it("does not flash when the check is rejected", async () => {
    checkMock.mockRejectedValue(new Error("offline"));
    await useUpdateStore.getState().checkForUpdate({ silent: true });
    expect(flashTaskbarMock).not.toHaveBeenCalled();
  });
});
