import { create } from "zustand";
import {
  setTelemetryEnabled,
  setOnboardingCompleted,
  setDefaultDownloadQuality,
  type DownloadQualityPreset,
  scanMusic,
} from "@/shared/lib/audio.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";

export type OnboardingStep = 1 | 2 | 3;

export interface OnboardingState {
  step: OnboardingStep;
  libraryDir: string | null;
  telemetryEnabled: boolean;
  downloadQuality: DownloadQualityPreset;
  isSaving: boolean;

  setStep: (step: OnboardingStep) => void;
  nextStep: () => void;
  prevStep: () => void;
  setLibraryDir: (dir: string | null) => void;
  setTelemetryEnabled: (enabled: boolean) => void;
  setDownloadQuality: (quality: DownloadQualityPreset) => void;
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
  downloadQuality: "best" as const,
  isSaving: false,
};

export const useOnboardingStore = create<OnboardingState>((set, get) => ({
  ...initialState,

  setStep: (step) => set({ step }),
  nextStep: () => set(({ step }) => ({ step: step === 1 ? 2 : 3 })),
  prevStep: () => set(({ step }) => ({ step: step === 3 ? 2 : 1 })),
  setLibraryDir: (libraryDir) => set({ libraryDir }),
  setTelemetryEnabled: (telemetryEnabled) => set({ telemetryEnabled }),
  setDownloadQuality: (downloadQuality) => set({ downloadQuality }),

  finishOnboarding: async () => {
    const { libraryDir, telemetryEnabled, downloadQuality, isSaving } = get();
    if (isSaving) return;
    set({ isSaving: true });

    try {
      if (libraryDir) {
        localStorage.setItem("library_dir", libraryDir);
      }
      await setTelemetryEnabled(telemetryEnabled);
      await setDefaultDownloadQuality(downloadQuality);
      await setOnboardingCompleted(true);
      useModalStore.getState().closeModal();

      if (libraryDir) {
        void scanMusic(libraryDir)
          .then(() => useLibraryStore.getState().fetchTabs())
          .catch((err) => {
            console.error("Failed to scan onboarding library:", err);
          });
      }
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
      downloadQuality: "best",
      isSaving: false,
    }),
}));
