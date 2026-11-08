import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * URL for displaying a cover image.
 *
 * Pass the size it is shown at (CSS px) to get a small downscaled copy from the
 * `thumb://` protocol (see src-tauri/src/thumbs.rs). Browsers decode images to
 * full-size bitmaps whatever their display size, so list covers should always
 * use this. Omit `cssPx` to get the original file. Remote URLs pass through.
 */
export function coverSrc(path: string, cssPx?: number): string {
  if (/^https?:\/\//i.test(path)) return path;
  if (!cssPx) return convertFileSrc(path);
  const px = Math.ceil(cssPx * (window.devicePixelRatio || 1));
  return `${convertFileSrc(path, "thumb")}?s=${px}`;
}
