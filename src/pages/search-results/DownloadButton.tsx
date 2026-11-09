import type { MouseEvent, ReactNode } from "react";
import clsx from "clsx";
import { BiDownload } from "react-icons/bi";
import { FaCheckCircle } from "react-icons/fa";
import {
  ItemType,
  useLibraryStore,
  type DownloadProgress,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import { flyToLibrary } from "@/shared/lib/flyAnimation.ts";
import { downloadPercent, isDownloadBusy } from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";

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

  const onDownload = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    const rect = e.currentTarget.getBoundingClientRect();
    flyToLibrary(rect, item.img);
    startDownload({
      platform: item.platform,
      downloadType: item.item_type,
      url: item.item_url_path,
      artistName:
        item.band_name ||
        (item.item_type === ItemType.Album ? item.name : "Unknown Artist"),
      title: item.name,
      coverUrl: item.img,
    }).catch((err) => console.error("Download failed", err));
  };

  let control: ReactNode;
  if (busy && progress) {
    control =
      progress.status === "completed" ? (
        <span
          className={clsx(
            styles.progressText,
            overlay && styles.progressBadge,
          )}
          title="Downloaded"
          aria-label="Downloaded"
        >
          <FaCheckCircle size={overlay ? 18 : 16} />
        </span>
      ) : (
        <span
          className={clsx(
            styles.progressText,
            overlay && styles.progressBadge,
          )}
          aria-live="polite"
        >
          {downloadPercent(progress)}%
        </span>
      );
  } else {
    control = (
      <button
        type="button"
        className={overlay ? styles.downloadBtn : styles.rowDownloadBtn}
        onClick={onDownload}
        title={`Download ${item.name}`}
        aria-label={`Download ${item.name}`}
      >
        <BiDownload size={20} />
      </button>
    );
  }

  if (!overlay) return control;
  return (
    <div className={clsx(styles.coverOverlay, busy && styles.coverOverlayPinned)}>
      {control}
    </div>
  );
};
