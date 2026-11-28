import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useOnboardingStore } from "./onboardingStore.ts";

const { fetchTabsMock, closeModalMock } = vi.hoisted(() => ({
  fetchTabsMock: vi.fn().mockResolvedValue(undefined),
  closeModalMock: vi.fn(),
}));

vi.mock("@/shared/lib/audio.ts", () => ({
  setDefaultDownloadQuality: vi.fn().mockResolvedValue(undefined),
  setTelemetryEnabled: vi.fn().mockResolvedValue(undefined),
  setOnboardingCompleted: vi.fn().mockResolvedValue(undefined),
  scanMusic: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/shared/store/libraryStore.ts", () => ({
  useLibraryStore: {
    getState: vi.fn(() => ({ fetchTabs: fetchTabsMock })),
  },
}));

vi.mock("@/shared/store/modalStore.ts", () => ({
  useModalStore: {
    getState: vi.fn(() => ({
      closeModal: closeModalMock,
    })),
  },
}));

describe("onboardingStore", () => {
  const store = new Map<string, string>();
  const mockStorage = {
    getItem: vi.fn((key: string) => store.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => store.set(key, String(value))),
    removeItem: vi.fn((key: string) => store.delete(key)),
    clear: vi.fn(() => store.clear()),
    get length() {
      return store.size;
    },
    key: vi.fn((_i: number) => null),
  };

  beforeEach(async () => {
    vi.clearAllMocks();
    fetchTabsMock.mockResolvedValue(undefined);
    const { scanMusic, setTelemetryEnabled, setOnboardingCompleted, setDefaultDownloadQuality } =
      await import("@/shared/lib/audio.ts");
    vi.mocked(scanMusic).mockResolvedValue([]);
    vi.mocked(setTelemetryEnabled).mockResolvedValue(undefined);
    vi.mocked(setOnboardingCompleted).mockResolvedValue(undefined);
    vi.mocked(setDefaultDownloadQuality).mockResolvedValue(undefined);
    store.clear();
    vi.stubGlobal("localStorage", mockStorage);
    useOnboardingStore.getState().reset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("initializes with step 1 and telemetry enabled by default", () => {
    const state = useOnboardingStore.getState();
    expect(state.step).toBe(1);
    expect(state.telemetryEnabled).toBe(true);
    expect(state.libraryDir).toBeNull();
    expect(state.isSaving).toBe(false);
  });

  it("navigates between steps", () => {
    useOnboardingStore.getState().nextStep();
    expect(useOnboardingStore.getState().step).toBe(2);

    useOnboardingStore.getState().prevStep();
    expect(useOnboardingStore.getState().step).toBe(1);
  });

  it("allows setting libraryDir and toggling telemetry", () => {
    useOnboardingStore.getState().setLibraryDir("/home/user/Music");
    expect(useOnboardingStore.getState().libraryDir).toBe("/home/user/Music");

    useOnboardingStore.getState().setTelemetryEnabled(false);
    expect(useOnboardingStore.getState().telemetryEnabled).toBe(false);
  });

  it("finishes onboarding and persists settings", async () => {
    const { setTelemetryEnabled, setOnboardingCompleted, scanMusic } = await import(
      "@/shared/lib/audio.ts"
    );

    useOnboardingStore.getState().setLibraryDir("/music/path");
    useOnboardingStore.getState().setTelemetryEnabled(true);

    await useOnboardingStore.getState().finishOnboarding();

    expect(localStorage.getItem("library_dir")).toBe("/music/path");
    expect(scanMusic).toHaveBeenCalledWith("/music/path");
    expect(setTelemetryEnabled).toHaveBeenCalledWith(true);
    expect(setOnboardingCompleted).toHaveBeenCalledWith(true);
    expect(closeModalMock).toHaveBeenCalled();
  });

  it("closes onboarding while the library scan is still pending", async () => {
    const { scanMusic, setTelemetryEnabled, setDefaultDownloadQuality, setOnboardingCompleted } =
      await import("@/shared/lib/audio.ts");
    let resolveScan!: (value: Awaited<ReturnType<typeof scanMusic>>) => void;
    vi.mocked(scanMusic).mockImplementationOnce(
      () => new Promise((resolve) => { resolveScan = resolve; }),
    );

    useOnboardingStore.getState().setLibraryDir("/music/path");
    let finishingResolved = false;
    const finishing = useOnboardingStore.getState().finishOnboarding();
    void finishing.then(() => { finishingResolved = true; });

    await vi.waitFor(() => {
      expect(closeModalMock).toHaveBeenCalledTimes(1);
      expect(finishingResolved).toBe(true);
      expect(setTelemetryEnabled).toHaveBeenCalledWith(true);
      expect(setDefaultDownloadQuality).toHaveBeenCalledWith("best");
      expect(setOnboardingCompleted).toHaveBeenCalledWith(true);
    });

    expect(useOnboardingStore.getState().isSaving).toBe(false);
    expect(localStorage.getItem("library_dir")).toBe("/music/path");
    expect(fetchTabsMock).not.toHaveBeenCalled();

    resolveScan([]);
    await finishing;
    await vi.waitFor(() => expect(fetchTabsMock).toHaveBeenCalledTimes(1));
  });

  it("keeps onboarding closed and does not fetch tabs when the background scan fails", async () => {
    const { scanMusic } = await import("@/shared/lib/audio.ts");
    const scanError = new Error("scan failed");
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    let rejectScan!: (reason?: unknown) => void;
    vi.mocked(scanMusic).mockImplementationOnce(
      () => new Promise((_resolve, reject) => { rejectScan = reject; }),
    );

    useOnboardingStore.getState().setLibraryDir("/music/path");
    const finishing = useOnboardingStore.getState().finishOnboarding();
    await vi.waitFor(() => expect(closeModalMock).toHaveBeenCalledTimes(1), { timeout: 100 });
    expect(useOnboardingStore.getState().isSaving).toBe(false);

    rejectScan(scanError);
    await finishing;
    await vi.waitFor(() => expect(errorSpy).toHaveBeenCalledWith(
      "Failed to scan onboarding library:", scanError,
    ));
    expect(closeModalMock).toHaveBeenCalledTimes(1);
    expect(fetchTabsMock).not.toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("catches a tab refresh failure after the background scan", async () => {
    const { scanMusic } = await import("@/shared/lib/audio.ts");
    vi.mocked(scanMusic).mockResolvedValueOnce([]);
    const refreshError = new Error("refresh failed");
    fetchTabsMock.mockRejectedValueOnce(refreshError);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    useOnboardingStore.getState().setLibraryDir("/music/path");

    await useOnboardingStore.getState().finishOnboarding();

    await vi.waitFor(() => expect(errorSpy).toHaveBeenCalledWith(
      "Failed to scan onboarding library:", refreshError,
    ));
    expect(fetchTabsMock).toHaveBeenCalledTimes(1);
    expect(closeModalMock).toHaveBeenCalledTimes(1);
    expect(useOnboardingStore.getState().isSaving).toBe(false);
    errorSpy.mockRestore();
  });

  it("does not close onboarding or start a scan when saving settings fails", async () => {
    const { scanMusic, setTelemetryEnabled } = await import("@/shared/lib/audio.ts");
    const saveError = new Error("settings failed");
    vi.mocked(setTelemetryEnabled).mockRejectedValueOnce(saveError);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    useOnboardingStore.getState().setLibraryDir("/music/path");

    await useOnboardingStore.getState().finishOnboarding();

    expect(scanMusic).not.toHaveBeenCalled();
    expect(closeModalMock).not.toHaveBeenCalled();
    expect(useOnboardingStore.getState().isSaving).toBe(false);
    expect(errorSpy).toHaveBeenCalledWith("Failed to complete onboarding:", saveError);
    errorSpy.mockRestore();
  });

  it("persists the selected quality from the new download step", async () => {
    const { setDefaultDownloadQuality } = await import("@/shared/lib/audio.ts");
    const state = useOnboardingStore.getState();
    state.nextStep();
    state.nextStep();
    expect(useOnboardingStore.getState().step).toBe(3);
    state.setDownloadQuality("standard");
    await state.finishOnboarding();
    expect(setDefaultDownloadQuality).toHaveBeenCalledWith("standard");
  });

  it("skips onboarding without setting library folder but marking completed", async () => {
    const { setTelemetryEnabled, setOnboardingCompleted, scanMusic } = await import(
      "@/shared/lib/audio.ts"
    );

    await useOnboardingStore.getState().skipOnboarding();

    expect(localStorage.getItem("library_dir")).toBeNull();
    expect(scanMusic).not.toHaveBeenCalled();
    expect(setTelemetryEnabled).toHaveBeenCalledWith(true);
    expect(setOnboardingCompleted).toHaveBeenCalledWith(true);
    expect(closeModalMock).toHaveBeenCalled();
  });

  it("finishes onboarding without scanning when no library folder is selected", async () => {
    const { scanMusic, setOnboardingCompleted } = await import("@/shared/lib/audio.ts");

    await useOnboardingStore.getState().finishOnboarding();

    expect(scanMusic).not.toHaveBeenCalled();
    expect(fetchTabsMock).not.toHaveBeenCalled();
    expect(setOnboardingCompleted).toHaveBeenCalledWith(true);
    expect(closeModalMock).toHaveBeenCalledTimes(1);
    expect(useOnboardingStore.getState().isSaving).toBe(false);
  });
});
