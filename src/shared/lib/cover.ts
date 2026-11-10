import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * URL for displaying a cover image.
 *
 * Pass the size it is shown at (CSS px) to get a small copy: local files go through the `thumb://` protocol
 * (see src-tauri/src/thumbs.rs), Bandcamp and Google (YouTube) images are asked from their server at a size close to
 * it. Browsers decode images to full-size bitmaps whatever their display size, and a Bandcamp original can weigh
 * 10 MB, so covers in lists and in the player should always pass it. Omit `cssPx` to get the original. Other remote
 * URLs pass through.
 */
export function coverSrc(path: string, cssPx?: number): string {
  const px = cssPx ? Math.ceil(cssPx * (window.devicePixelRatio || 1)) : undefined;
  if (/^https?:\/\//i.test(path)) return px ? sizedRemoteCover(path, px) : path;
  if (!px) return convertFileSrc(path);
  return `${convertFileSrc(path, "thumb")}?s=${px}`;
}

/** Square Bandcamp album art formats: [side in px, format id], smallest first. */
const BANDCAMP_SIZES: ReadonlyArray<readonly [number, number]> = [
  [50, 42],
  [100, 3],
  [150, 7],
  [200, 44],
  [350, 2],
  [700, 16],
  [1200, 10],
];

const BANDCAMP_ART = /^(https?:\/\/f\d*\.bcbits\.com\/img\/a\d+)_\d+\.(?:jpe?g|png)(?=$|[?#])/i;
const GOOGLE_IMAGE = /^https?:\/\/(?:lh\d+|yt\d+)\.googleusercontent\.com\//i;
const GOOGLE_SIZE = /=w\d+-h\d+/;

/**
 * The same remote cover at a size of at least `px` device pixels, when the host serves sizes (Bandcamp album art,
 * Google images with a `=wN-hN` suffix); any other URL unchanged. Pure.
 */
export function sizedRemoteCover(url: string, px: number): string {
  const bandcamp = url.match(BANDCAMP_ART);
  if (bandcamp) {
    const format = (BANDCAMP_SIZES.find(([side]) => side >= px) ?? BANDCAMP_SIZES[BANDCAMP_SIZES.length - 1])[1];
    return `${bandcamp[1]}_${format}.jpg${url.slice(bandcamp[0].length)}`;
  }
  if (GOOGLE_IMAGE.test(url) && GOOGLE_SIZE.test(url)) {
    const side = Math.max(1, Math.ceil(px));
    return url.replace(GOOGLE_SIZE, `=w${side}-h${side}`);
  }
  return url;
}
