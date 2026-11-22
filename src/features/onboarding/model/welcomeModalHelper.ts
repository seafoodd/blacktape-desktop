export const PRIVACY_BULLETS = [
  "Randomized install ID (no hardware, IP, or machine fingerprinting)",
  "No track names, audio files, or personal paths are ever sent",
  "You can change this or opt out anytime in Settings > About",
] as const;

export function stepBadge(step: 1 | 2): string {
  return step === 1
    ? "Step 1 of 2 · Music Library"
    : "Step 2 of 2 · Privacy & Diagnostics";
}

export function formatFolderDisplay(folder: string | null | undefined): string {
  if (!folder || folder.trim() === "") {
    return "No music folder selected";
  }
  return folder;
}

export function stepTitle(step: 1 | 2): string {
  return step === 1 ? "Welcome to Blacktape" : "Help Improve Blacktape";
}

export function stepSubtitle(step: 1 | 2): string {
  return step === 1
    ? "Fast, local music player and streamer"
    : "Crash reports & anonymous diagnostics";
}
