import { create } from "zustand";
import {
  setTelemetryEnabled,
  setOnboardingCompleted,
  scanMusic,
} from "@/shared/lib/audio.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";

export interface OnboardingState {
  step: 1 | 2;
  libraryDir: string | null;
  telemetryEnabled: boolean;
  isSaving: boolean;

  setStep: (step: 1 | 2) => void;
  nextStep: () => void;
  prevStep: () => void;
  setLibraryDir: (dir: string | null) => void;
  setTelemetryEnabled: (enabled: boolean) => void;
  finishOnboarding: () => Promise<void>;
  skipOnboarding: () => Promise<void>;
  reset: () => void;
}

const getInitialLibraryDir = (): string | null => {
  if (typeof window !== "undefined" && typeof localStorage !== "undefined") {
    try {
      return localStorage.getItem("library_dir") || null;
    } catch {
      return null;
    }
  }
  return null;
};

const initialState = {
  step: 1 as const,
  libraryDir: getInitialLibraryDir(),
  telemetryEnabled: true,
  isSaving: false,
};

export const useOnboardingStore = create<OnboardingState>((set, get) => ({
  ...initialState,

  setStep: (step) => set({ step }),
  nextStep: () => set((state) => ({ step: state.step === 1 ? 2 : state.step })),
  prevStep: () => set((state) => ({ step: state.step === 2 ? 1 : state.step })),
  setLibraryDir: (libraryDir) => set({ libraryDir }),
  setTelemetryEnabled: (telemetryEnabled) => set({ telemetryEnabled }),

  finishOnboarding: async () => {
    const { libraryDir, telemetryEnabled, isSaving } = get();
    if (isSaving) return;
    set({ isSaving: true });

    try {
      if (libraryDir) {
        localStorage.setItem("library_dir", libraryDir);
        await scanMusic(libraryDir);
        await useLibraryStore.getState().fetchTabs();
      }
      await setTelemetryEnabled(telemetryEnabled);
      await setOnboardingCompleted(true);
      useModalStore.getState().closeModal();
    } catch (err) {
      console.error("Failed to complete onboarding:", err);
    } finally {
      set({ isSaving: false });
    }
  },

  skipOnboarding: async () => {
    const { telemetryEnabled, isSaving } = get();
    if (isSaving) return;
    set({ isSaving: true });

    try {
      await setTelemetryEnabled(telemetryEnabled);
      await setOnboardingCompleted(true);
      useModalStore.getState().closeModal();
    } catch (err) {
      console.error("Failed to skip onboarding:", err);
    } finally {
      set({ isSaving: false });
    }
  },

  reset: () =>
    set({
      step: 1,
      libraryDir: getInitialLibraryDir(),
      telemetryEnabled: true,
      isSaving: false,
    }),
}));
