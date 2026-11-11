import type { DownloadProgress, StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { sameUrl } from "@/shared/lib/urlKey.ts";

type Download = Pick<DownloadProgress, "url" | "status" | "trackUrls">;

/**
 * The download in `active` that already covers `params`, unless it failed or was cancelled (a completed one is
 * returned only when none still runs: its songs are in the library). It covers them when it is for the same page
 * and it downloads the whole page (no track list: a whole album, a single track, or one recovered after a restart)
 * or every one of the requested tracks; a download of some tracks does not cover the whole album. Pure.
 */
export function findDownload<D extends Download>(
  active: Record<string, D>,
  params: Pick<StartDownloadParams, "url" | "trackUrls">,
): D | undefined {
  const covering = Object.values(active).filter(
    (d) =>
      d.status !== "failed" &&
      d.status !== "cancelled" &&
      sameUrl(d.url, params.url) &&
      (!d.trackUrls ||
        (params.trackUrls !== undefined &&
          params.trackUrls.every((url) => d.trackUrls!.some((have) => sameUrl(have, url))))),
  );
  return covering.find(isRunning) ?? covering[0];
}

/** True while `download` still runs (it neither finished nor failed nor was cancelled). Pure. */
export function isRunning(download: Pick<DownloadProgress, "status"> | undefined): boolean {
  return (
    download !== undefined &&
    download.status !== "completed" &&
    download.status !== "failed" &&
    download.status !== "cancelled"
  );
}
