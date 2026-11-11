/** Window background opacity when nothing valid is stored: a fresh install starts fully opaque. */
export const DEFAULT_BG_OPACITY = 1;

/** Blur of what is behind the window when nothing valid is stored: off. */
export const DEFAULT_BG_BLUR = false;

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
 * Reads the stored blur toggle: "true" or "false". Older versions stored a 0–1 slider value, so a
 * number above 0 also counts as on and 0 as off. Anything else gives `fallback`.
 */
export function parseBlurSetting(saved: string | null, fallback: boolean): boolean {
  if (saved === null) return fallback;
  const text = saved.trim();
  if (text === "true") return true;
  if (text === "false") return false;
  if (text === "") return fallback;
  const value = Number(text);
  return Number.isFinite(value) ? value > 0 : fallback;
}

/**
 * Whether the native blur effect should be on. It only shows through a see-through window, and it
 * makes dragging and resizing slower on some Windows builds, so it stays off at full opacity.
 */
export function windowBlurEnabled(opacity: number, blur: boolean): boolean {
  return opacity < 1 && blur;
}

/** "#RRGGBB" (or "#RGB") to [r, g, b]; anything else gives black. */
export function hexToRgb(hex: string): [number, number, number] {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return [0, 0, 0];
  const digits =
    match[1].length === 3
      ? [...match[1]].map((d) => d + d).join("")
      : match[1];
  return [0, 2, 4].map((i) => parseInt(digits.slice(i, i + 2), 16)) as [
    number,
    number,
    number,
  ];
}
