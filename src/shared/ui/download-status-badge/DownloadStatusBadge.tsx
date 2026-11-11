import clsx from "clsx";
import { FaCheck, FaCheckCircle } from "react-icons/fa";
import type { DownloadState } from "@/shared/lib/library/libraryIndex.ts";
import { downloadStatusLabel } from "./downloadStatusLabel.ts";
import styles from "./download-status-badge.module.css";

export interface DownloadStatusBadgeProps {
  state: DownloadState;
  /** Icon size in px (default 16). */
  size?: number;
  className?: string;
}

/**
 * "In the library" marker: a filled check for `downloaded`, the same check inside a dashed ring for
 * `partial`, nothing for `none`. The box is `size` square in both cases so rows and covers line up.
 */
export const DownloadStatusBadge = ({ state, size = 16, className }: DownloadStatusBadgeProps) => {
  const label = downloadStatusLabel(state);
  if (label === null) return null;
  const partial = state === "partial";
  return (
    <span
      className={clsx(styles.badge, partial && styles.partial, className)}
      style={{ width: size, height: size }}
      title={label}
      aria-label={label}
      role="img"
    >
      {partial ? <FaCheck size={Math.round(size * 0.5)} /> : <FaCheckCircle size={size} />}
    </span>
  );
};
