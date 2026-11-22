export const TELEMETRY_COPY = {
  title: "Privacy & Telemetry",
  description:
    "Blacktape collects anonymous crash reports and diagnostics so issues can be fixed quickly. No personal data, audio files, or track names are sent.",
} as const;

export function telemetryToggleLabel(enabled: boolean): string {
  return enabled
    ? "Send anonymous crash reports & diagnostics"
    : "Send anonymous crash reports & diagnostics (disabled)";
}
