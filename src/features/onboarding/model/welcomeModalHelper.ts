import type { OnboardingStep } from "./onboardingStore.ts";

export const PRIVACY_BULLETS = [
  "Randomized install ID (no hardware, IP, or machine fingerprinting)",
  "No track names, audio files, or personal paths are ever sent",
  "You can change this or opt out anytime in Settings > About",
] as const;

export function stepBadge(step: OnboardingStep): string {
  return `Step ${step} of 3 · ${["Music Library", "Privacy & Diagnostics", "Downloads"][step - 1]}`;
}

export function formatFolderDisplay(folder: string | null | undefined): string {
  if (!folder || folder.trim() === "") {
    return "No music folder selected";
  }
  return folder;
}

export function stepTitle(step: OnboardingStep): string {
  return ["Welcome to Blacktape", "Help Improve Blacktape", "Default Download Quality"][step - 1];
}

export function stepSubtitle(step: OnboardingStep): string {
  return ["Fast, local music player and streamer", "Crash reports & anonymous diagnostics",
    "Choose what Quick Download looks for first"][step - 1];
}
