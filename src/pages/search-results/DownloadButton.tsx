import type { MouseEvent, ReactNode } from "react";
import clsx from "clsx";
import { FaCheckCircle } from "react-icons/fa";
import {
  ItemType,
  useLibraryStore,
  type DownloadProgress,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import { flyToLibrary } from "@/shared/lib/ui/flyAnimation.ts";
import { remoteRefOf, useAlbumStatus, useTrackStatus } from "@/shared/hooks/useDownloadStatus.ts";
import { downloadPercent, isDownloadBusy, searchItemDownloadParams } from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";
import { DownloadSplitButton } from "@/shared/ui/download-split-button/DownloadSplitButton.tsx";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";

interface DownloadButtonProps {
  item: SearchSuggestion;
  /** Download of this item (matched by url by the page), if any. */
  progress?: DownloadProgress;
  /** "overlay": round button on top of a cover (hover reveal). "row": inline icon button of a list row. */
  variant: "overlay" | "row";
}

/**
 * Download control of one item, same behaviour as the search dropdown: download button, then percent while
 * running, a check when completed; failed and cancelled downloads show the button again.
 */
export const DownloadButton = ({ item, progress, variant }: DownloadButtonProps) => {
  const startDownload = useLibraryStore((s) => s.startDownload);
  const busy = isDownloadBusy(progress);
  const overlay = variant === "overlay";
  // Items already in the library show a check instead of a button that would only download them again.
  const isAlbum = item.item_type === ItemType.Album;
  const ref = remoteRefOf(item);
  const trackState = useTrackStatus(isAlbum ? null : ref);
  const albumState = useAlbumStatus(isAlbum ? ref : null).state;
  const inLibrary = (isAlbum ? albumState : trackState) === "downloaded";
  // a partly downloaded album: the dashed check at rest, the download button (for the rest) on hover
  const partial = overlay && isAlbum && albumState === "partial" && !busy;

  const onDownload = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    const rect = e.currentTarget.getBoundingClientRect();
    flyToLibrary(rect, item.img);
    startDownload(searchItemDownloadParams(item)).catch((err) => console.error("Download failed", err));
  };

  let control: ReactNode;
  if (busy && progress) {
    control =
      progress.status === "completed" ? (
        // a row shows the same check as a song already in the library, in the same place
        <span
          className={clsx(overlay ? [styles.progressText, styles.progressBadge] : styles.libraryMark)}
          title="Downloaded"
          aria-label="Downloaded"
        >
          <FaCheckCircle size={18} />
        </span>
      ) : (
        <span
          className={clsx(styles.progressText, overlay ? styles.progressBadge : styles.progressRow)}
          aria-live="polite"
        >
          {downloadPercent(progress)}%
        </span>
      );
  } else if (inLibrary) {
    control = (
      <span
        className={clsx(overlay ? [styles.progressText, styles.progressBadge] : styles.libraryMark)}
        title="Already in your library"
        aria-label="Already in your library"
      >
        <FaCheckCircle size={18} />
      </span>
    );
  } else {
    control = (
      <DownloadSplitButton
        size={overlay ? 34 : 32}
        tone={overlay ? "solid" : "plain"}
        className={overlay ? undefined : styles.rowSplit}
        title={`Download ${item.name}`}
        onDownload={onDownload}
        onChooseQuality={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          openQualityMenu(searchItemDownloadParams(item), { onStarted: () => flyToLibrary(rect, item.img) });
        }}
      />
    );
  }

  // a fixed-width cell: the columns before it line up whether a row shows the button, a check or a percent
  if (!overlay) return <span className={styles.rowDownloadCell}>{control}</span>;
  return (
    // the check of an album in the library stays visible; the download button shows on hover
    <div
      className={clsx(
        styles.coverOverlay,
        (busy || inLibrary || partial) && styles.coverOverlayPinned,
        partial && styles.coverOverlayPartial,
      )}
    >
      {partial && (
        <span className={clsx(styles.progressText, styles.progressBadge, styles.partialMark)}>
          <DownloadStatusBadge state="partial" size={18} className={styles.partialBadge} />
        </span>
      )}
      {control}
    </div>
  );
};
