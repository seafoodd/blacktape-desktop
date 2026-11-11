import { setWindowBlur, windowBlurSupported } from "@/shared/lib/audio.ts";

/**
 * Turns the native blur behind the current window on or off (acrylic on Windows, vibrancy on
 * macOS), tinted with the theme background `tint` on Windows. The OS uses a fixed blur radius, so
 * this is on/off. Does nothing where the platform has no window effects; a failure only leaves
 * the window unblurred.
 */
export function applyWindowBlur(
  enabled: boolean,
  tint: [number, number, number],
): void {
  windowBlurSupported()
    .then((supported) => (supported ? setWindowBlur(enabled, tint) : undefined))
    .catch((err) => console.error("Failed to change window blur:", err));
}
