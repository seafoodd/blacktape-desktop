import type { CSSProperties, MouseEvent, ReactNode } from "react";
import clsx from "clsx";
import { BiDownload } from "react-icons/bi";
import { MdSettings } from "react-icons/md";
import styles from "./download-split-button.module.css";

interface DownloadSplitButtonProps {
  /** Downloads in the best quality. */
  onDownload: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Opens the quality menu. */
  onChooseQuality: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Tooltip / accessible name of the download half ("Download Paranoid"). */
  title: string;
  disabled?: boolean;
  /** Height in px (the icon scales with it); default 32. */
  size?: number;
  /** "plain": transparent until hovered (rows, player). "solid": dark backdrop (on top of a cover). */
  tone?: "plain" | "solid";
  icon?: ReactNode;
  /** The gear is hidden at rest and slides out to the right of the icon on hover (the player). */
  revealGear?: boolean;
  className?: string;
}

/**
 * One rounded button with two halves, the download icon and a small gear. On hover both halves light up, split
 * by a narrow gap with sharp corners: the left half downloads in the best quality, the gear opens the quality menu.
 */
export const DownloadSplitButton = ({
  onDownload,
  onChooseQuality,
  title,
  disabled,
  size = 32,
  tone = "plain",
  icon,
  revealGear,
  className,
}: DownloadSplitButtonProps) => (
  <div
    className={clsx(styles.split, tone === "solid" && styles.solid, revealGear && styles.reveal, disabled && styles.disabled, className)}
    style={{ "--split-size": `${size}px` } as CSSProperties}
    role="group"
    aria-label={title}
  >
    <button
      type="button"
      className={styles.main}
      onClick={(e) => {
        e.stopPropagation();
        onDownload(e);
      }}
      disabled={disabled}
      title={title}
      aria-label={title}
    >
      {icon ?? <BiDownload />}
    </button>
    <button
      type="button"
      className={styles.arrow}
      onClick={(e) => {
        e.stopPropagation();
        onChooseQuality(e);
      }}
      disabled={disabled}
      title="Choose the download quality"
      aria-label="Choose the download quality"
      aria-haspopup="dialog"
    >
      <MdSettings />
    </button>
  </div>
);
