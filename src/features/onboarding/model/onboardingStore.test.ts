import { describe, it, expect, vi, beforeEach } from "vitest";
import { useOnboardingStore } from "./onboardingStore.ts";

vi.mock("@/shared/lib/audio.ts", () => ({
  setTelemetryEnabled: vi.fn().mockResolvedValue(undefined),
  setOnboardingCompleted: vi.fn().mockResolvedValue(undefined),
  scanMusic: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/shared/store/libraryStore.ts", () => ({
  useLibraryStore: {
    getState: vi.fn(() => ({
      fetchTabs: vi.fn().mockResolvedValue(undefined),
    })),
  },
}));

const closeModalMock = vi.fn();
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

  beforeEach(() => {
    vi.clearAllMocks();
    store.clear();
    vi.stubGlobal("localStorage", mockStorage);
    useOnboardingStore.getState().reset();
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
});
