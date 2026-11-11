/** Window background opacity when nothing valid is stored: a fresh install starts fully opaque. */
export const DEFAULT_BG_OPACITY = 1;

/** Blur of what is behind the window when nothing valid is stored: off. */
export const DEFAULT_BG_BLUR = 0;

/**
 * Reads a 0–1 setting stored as text in localStorage. Missing, empty or non-numeric values give
 * `fallback`; numbers outside 0–1 are clamped.
 */
export function parseUnitSetting(saved: string | null, fallback: number): number {
  if (saved === null || saved.trim() === "") return fallback;
  const value = Number(saved);
  if (!Number.isFinite(value)) return fallback;
  return Math.min(1, Math.max(0, value));
}

/**
 * Whether the native blur effect should be on. It only shows through a see-through window, and it
 * makes dragging and resizing slower on some Windows builds, so it stays off at full opacity.
 */
export function windowBlurEnabled(opacity: number, blur: number): boolean {
  return opacity < 1 && blur > 0;
}
